//! **The crate root: focus cards.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    THREE_LINES, a_local_file, a_local_folder, card_restore_first, card_restore_fixture,
    card_restore_resize, card_restore_settle, card_restore_widen, host_file_uri, host_path,
    paste_leaf, paste_tab, paste_text_into, squeezed_body,
};
use std::time::Duration;
use winit::keyboard::{Key, NamedKey};

// ── one file, one card, from wherever you point at it (2026-08-27) ─────

/// PIN — **a reference printed in the terminal raises the card the files
/// column raises, and a remote address raises nothing.**
///
/// The ruling's first half as a table: *一个文件,不论从哪指向它,都是同一
/// 张卡*. What is being pinned is not really the mapping — it is that the
/// mapping is [`hyperlink_activation`]'s and not a second one: §7.1.5j ①
/// folded every printed shape of a local file into one `file:` link fed to
/// one routing table, and a hover that judged for itself which references
/// are files would drift from the click that opens them. The first symptom
/// of that drift is a card standing over something a press does nothing to.
///
/// MUTATIONS that must turn it red:
/// ① give the directory arm to the glance (`Preview` and `FilesColumn` both
///    to [`ReferenceCard::File`]) — a folder gets a document card that can
///    only refuse it, and the flyout that *is* a folder's card never opens;
/// ② answer a card for [`HyperlinkActivation::Page`] — a remote address
///    raises a card this window cannot fill. It is `Page` and **not**
///    `Browser` that has to stay silent, which is worth knowing: `Browser`
///    is `Ctrl`'s answer and is unreachable at `control: false`, so since
///    2026-08-29 a remote address arrives down the plain half as a page —
///    an arm that *acts*, and still has no card, because both of this
///    window's cards are made of a file on this disk. The arms are spelled
///    out rather than folded into a wildcard so that a future reader has to
///    decide about them rather than inherit a `_`;
/// ③ pass `control: true` — the table's other half is the system's, so
///    every local file stops raising a card the moment `Ctrl` is held down.
#[test]
fn a_reference_in_the_output_raises_the_card_the_files_column_raises() {
    let file = |_: &Path| Some(a_local_file());
    let folder = |_: &Path| Some(a_local_folder());

    // A file, however it was printed: §7.1.5j turns a bare path, a `file:`
    // URI and an OSC 8 target into the same link, so one case covers all
    // three by construction.
    assert_eq!(
        reference_card(
            &host_file_uri(r"C:\Developer\notes.md"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &file
        ),
        Some(ReferenceCard::File(host_path(r"C:\Developer\notes.md")))
    );
    // And every class the card has a body for arrives down that same arm —
    // which is the point: the lane is chosen from the *path*, by the one
    // reader ([`peek_body_kind`]) both hosts go through, and never here.
    for name in ["report.pdf", "page.html", "shot.png", "clip.mp4", "a.bin"] {
        let uri = host_file_uri(&format!(r"C:\Developer\{name}"));
        assert!(
            matches!(
                reference_card(&uri, bt_transcript::paths::PathNamer::ThisWindow, &file),
                Some(ReferenceCard::File(_))
            ),
            "{name} is a file, and the card decides what to draw of it elsewhere"
        );
    }

    // A folder — including one named like a page, because the directory
    // question is asked before the page question and was settled first.
    assert_eq!(
        reference_card(
            &host_file_uri(r"C:\Developer\src"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &folder
        ),
        Some(ReferenceCard::Folder(host_path(r"C:\Developer\src")))
    );
    assert!(matches!(
        reference_card(
            &host_file_uri(r"C:\Developer\site.html"),
            bt_transcript::paths::PathNamer::ThisWindow,
            &folder
        ),
        Some(ReferenceCard::Folder(_))
    ));

    // A share is a file to this door, and the card it raises prints §7.1.3's
    // refusal — the preview's judgement borrowed, exactly as the files
    // column borrows it. The disk is never asked: `is_directory` would stall
    // the loop on a cold server, and the arm above it returns first. Off
    // Windows a `file:` URI naming another host names no path on this
    // machine, so it raises no card — and asks the disk no more than the
    // share does.
    let share_card = match bt_platform::host_platform() {
        bt_platform::HostPlatform::Windows => Some(ReferenceCard::File(PathBuf::from(
            r"\\server\share\notes.md",
        ))),
        bt_platform::HostPlatform::MacOs | bt_platform::HostPlatform::OtherUnix => None,
    };
    assert_eq!(
        reference_card(
            "file://server/share/notes.md",
            bt_transcript::paths::PathNamer::ThisWindow,
            &|_| { panic!("a share is answered without touching the network") }
        ),
        share_card
    );

    // **And nothing at all for what this window has no card of.** A remote
    // address opens on a plain click and still raises nothing: a card is
    // built from a file, and the address names none. A scheme with no arm at
    // all says nothing for the older reason — there is nowhere for it to go.
    for uri in [
        "https://example.com/report.pdf",
        "http://example.com",
        "mailto:someone@example.com",
        "notascheme",
    ] {
        assert_eq!(
            reference_card(uri, bt_transcript::paths::PathNamer::ThisWindow, &file),
            None,
            "{uri} has no destination inside this window, so it has no card"
        );
    }
}

/// One seat, named the way a card aim names one.
fn aim_seat(tab: u64, seat: u64) -> LeafId {
    LeafId {
        tab: TabId(tab),
        seat: SeatId(seat),
    }
}

/// A driver that reports travel rather than detents, `y` pixels of it.
fn wheel_pixels(y: f64) -> MouseScrollDelta {
    MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, y))
}

/// **A sixth of a detent, six times, is one row** (user report 2026-08-21:
/// "turning up works, but I have to turn for ages before it moves").
///
/// A high-resolution wheel and a precision touchpad both report one detent
/// as a run of small travels — 20 pixels at a time against Win32's 120 —
/// and each of those on its own rounds to no rows at all. Rounding each
/// event in isolation throws the whole turn away, which is what the report
/// is describing; the fraction is *carried* instead, and the row falls the
/// moment the sixth nudge completes the detent.
#[test]
fn six_sixths_of_a_detent_add_up_to_one_row() {
    let mut aim = None;
    let seat = aim_seat(1, 1);
    let steps: Vec<i32> = (0..6)
        .map(|_| CardAim::spend(&mut aim, seat, wheel_pixels(20.0)))
        .collect();
    assert_eq!(
        steps,
        vec![0, 0, 0, 0, 0, 1],
        "a detent delivered in six pieces moves exactly one row, on the piece that completes it"
    );
}

/// The standard mouse is not made slower by the carry: a detent that arrives
/// whole is one row on arrival, and a merged burst is worth its own count.
#[test]
fn a_whole_detent_is_one_row_the_moment_it_lands() {
    let mut aim = None;
    let seat = aim_seat(1, 1);
    assert_eq!(
        CardAim::spend(&mut aim, seat, MouseScrollDelta::LineDelta(0.0, 1.0)),
        1
    );
    assert_eq!(
        CardAim::spend(&mut aim, seat, MouseScrollDelta::LineDelta(0.0, 1.0)),
        1
    );
    assert_eq!(
        CardAim::spend(&mut aim, seat, MouseScrollDelta::LineDelta(0.0, 3.0)),
        3,
        "a flick merged into one burst is worth every detent in it"
    );
    assert_eq!(
        CardAim::spend(&mut aim, seat, wheel_pixels(120.0)),
        1,
        "and a driver reporting travel says the same thing in its own currency"
    );
}

/// **A hand that changes its mind does not get the fraction it was owed.**
///
/// Half a detent upward is a promise about *up*; spending it on the way down
/// would make the first row down arrive early or late depending on something
/// the reader did before and cannot see.
#[test]
fn turning_the_other_way_forgets_the_fraction_it_was_owed() {
    let mut aim = None;
    let seat = aim_seat(1, 1);
    assert_eq!(CardAim::spend(&mut aim, seat, wheel_pixels(60.0)), 0);
    assert_eq!(
        CardAim::spend(&mut aim, seat, wheel_pixels(-120.0)),
        -1,
        "a whole detent down is one row down, not half of one"
    );
    assert_eq!(CardAim::spend(&mut aim, seat, wheel_pixels(-60.0)), 0);
    assert_eq!(
        CardAim::spend(&mut aim, seat, wheel_pixels(-60.0)),
        -1,
        "and the downward halves add up among themselves"
    );
}

/// **The fraction belongs to the seat it was turned at**, not to the window.
///
/// Moving the pointer to another seat — or to another card — starts that
/// seat's aim from nothing, because a carry that followed the pointer would
/// move a window the reader never turned the wheel over.
#[test]
fn the_fraction_belongs_to_the_seat_it_was_turned_at() {
    let mut aim = None;
    let half = wheel_pixels(60.0);
    assert_eq!(CardAim::spend(&mut aim, aim_seat(1, 1), half), 0);
    assert_eq!(
        CardAim::spend(&mut aim, aim_seat(1, 2), half),
        0,
        "the seat next door did not inherit the half detent"
    );
    assert_eq!(CardAim::spend(&mut aim, aim_seat(1, 2), half), 1);
    assert_eq!(
        CardAim::spend(&mut aim, aim_seat(2, 1), half),
        0,
        "and neither did the same-numbered seat on another card"
    );
    assert_eq!(CardAim::spend(&mut aim, aim_seat(2, 1), half), 1);
}

/// PIN — B22. The resizing cards pull in over exactly a hundred milliseconds
/// on CSS `ease`, and draw nothing at all on the frame the grab lands.
///
/// `.pane { transition: margin .1s ease, border-radius .1s ease, box-shadow
/// .1s ease }` (mock-up 1464) serving `.slot.resizing .pane { margin: 5px;
/// border-radius: 8px }` (1465-1470). Three halves of that declaration are
/// load-bearing and each is pinned against the way it gets quietly lost: the
/// **span** (borrow the pane FLIP's 200ms and the cards are still arriving
/// after the seam has been dragged somewhere else), the **curve** (linear is
/// a different gesture — it starts at full speed, which reads as a snap that
/// then decelerates), and the **first frame** (both drawn numbers are floored
/// at one physical pixel, so a card scaled by zero is a hairline of floor
/// around a pane nobody is resizing).
#[test]
fn a_grabbed_dividers_cards_pull_in_over_a_hundred_milliseconds_on_ease() {
    let now = Instant::now();
    let mut cards = RevealTween::over(RESIZING_CARD_TRANSITION);
    cards.retarget(1.0, now, Motion::Full);

    assert_eq!(
        cards.sample(now, Motion::Full).0,
        0.0,
        "on the frame the button goes down the panes are still flush"
    );
    assert_eq!(
        seats::resizing_card_inset(1.0, cards.sample(now, Motion::Full).0),
        None,
        "and nothing is drawn for them — a one-pixel inset is a visible line \
             around a pane that has not moved"
    );

    let span = RESIZING_CARD_TRANSITION.as_millis() as u64;
    for ms in [20_u64, 45, 70] {
        let at = now + Duration::from_millis(ms);
        let (inset, moving) = cards.sample(at, Motion::Full);
        assert!(
            moving,
            "{ms}ms into a {span}ms transition it is still running"
        );
        let want = cubic_bezier(ms as f32 / span as f32, EASE);
        assert!(
            (inset - want).abs() < 1e-6,
            "{ms}ms in the inset is {inset} where CSS `ease` says {want}"
        );
        let (margin, radius) =
            seats::resizing_card_inset(1.0, inset).expect("a running card is a card");
        assert!(
            margin <= bt_render::SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX
                && radius <= bt_render::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX,
            "and it never overshoots the declaration: {margin}, {radius}"
        );
    }

    // `ease` has a long flat tail, so the last frames of the transition round
    // to the very pixels it lands on — which is why the frame debt is settled
    // on the *drawn* inset. The frames that are genuinely part way in are the
    // early ones, and those are asserted rather than the tail.
    let (margin, radius) = seats::resizing_card_inset(
        1.0,
        cards
            .sample(now + Duration::from_millis(20), Motion::Full)
            .0,
    )
    .expect("a card twenty milliseconds in is a card");
    assert!(
        margin < bt_render::SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX
            && radius < bt_render::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX,
        "twenty milliseconds in the card has not arrived: {margin}, {radius}"
    );

    let landed = now + RESIZING_CARD_TRANSITION;
    assert_eq!(
        cards.sample(landed, Motion::Full),
        (1.0, false),
        "a hundred milliseconds is the whole of it, and the loop may sleep"
    );
    assert_eq!(
        seats::resizing_card_inset(1.0, cards.sample(landed, Motion::Full).0),
        Some((
            bt_render::SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX,
            bt_render::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX
        )),
        "landing on exactly the 5px and 8px F63 always drew"
    );
    assert_ne!(
        RESIZING_CARD_TRANSITION, PANE_FLIP,
        "a card inset answering a button press and half the window changing \
             shape cannot be one number"
    );
}

/// PIN — B22. A grab reversed mid-transition turns around from where the
/// cards actually are, not from an end they never reached.
///
/// The fourth user of [`RevealTween`] and the first one that is genuinely
/// interrupted in ordinary use: a hand that grabs a divider, thinks better of
/// it and lets go inside the hundred milliseconds. A CSS transition
/// interrupted mid-flight restarts from the computed value, which is what
/// `retarget` gives — restart from 1.0 instead and the cards jump *in* the
/// rest of the way before running out, which is a flinch rather than a
/// reversal.
#[test]
fn a_card_transition_reversed_mid_flight_turns_around_from_where_it_is() {
    let now = Instant::now();
    let mut cards = RevealTween::over(RESIZING_CARD_TRANSITION);
    cards.retarget(1.0, now, Motion::Full);

    let at = now + Duration::from_millis(40);
    let (reached, _) = cards.sample(at, Motion::Full);
    assert!(
        reached > 0.0 && reached < 1.0,
        "40ms of 100ms is genuinely mid-flight: {reached}"
    );

    cards.retarget(0.0, at, Motion::Full);
    assert!(
        (cards.sample(at, Motion::Full).0 - reached).abs() < 1e-6,
        "the reversal opens on the inset the cards had, {reached}, and not on \
             the 1.0 they were aimed at"
    );
    assert!(
        cards.sample(at + Duration::from_millis(10), Motion::Full).0 < reached,
        "and runs down from there"
    );
    assert_eq!(
        cards.sample(at + RESIZING_CARD_TRANSITION, Motion::Full),
        (0.0, false),
        "reaching flush a hundred milliseconds after the hand opened"
    );
}

/// PIN — B22 under reduced motion: the cards appear and vanish at once, and
/// no frame is owed for either.
///
/// Verified rather than assumed. "There is no transition under Reduced" is a
/// property of [`RevealTween::retarget`] storing no `started`, and the two
/// things that follow from it — the terminal inset on the first sample, and
/// a `false` that lets `strip_animation_work` ask for no deadline at all —
/// are each a separate consumer that could have read the clock for itself.
#[test]
fn reduced_motion_snaps_the_resizing_cards_in_and_out_and_asks_for_no_frames() {
    let now = Instant::now();
    let mut cards = RevealTween::over(RESIZING_CARD_TRANSITION);

    cards.retarget(1.0, now, Motion::Reduced);
    assert_eq!(
        cards.sample(now, Motion::Reduced),
        (1.0, false),
        "the cards are simply there, and nothing is asking to be woken"
    );
    assert_eq!(
        seats::resizing_card_inset(1.0, cards.sample(now, Motion::Reduced).0),
        Some((
            bt_render::SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX,
            bt_render::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX
        )),
    );

    cards.retarget(0.0, now, Motion::Reduced);
    assert_eq!(
        cards.sample(now, Motion::Reduced),
        (0.0, false),
        "and simply gone"
    );
    assert_eq!(
        seats::resizing_card_inset(1.0, cards.sample(now, Motion::Reduced).0),
        None
    );
}

#[test]
fn card_restore_extra_upward_detent_stays_at_top() {
    let mut leaf = card_restore_fixture();
    card_restore_widen(&mut leaf);
    let before = card_restore_first(&leaf);
    let mut carry = None;
    let steps = CardAim::spend(
        &mut carry,
        LeafId {
            tab: TabId(1),
            seat: SeatId(1),
        },
        MouseScrollDelta::LineDelta(0.0, 1.0),
    );
    aim_card_window(&mut leaf, 4, steps, card_trace::Card::untraced());
    let after = card_restore_first(&leaf);
    eprintln!("upward projection: {before} -> {after}");
    assert_eq!(after, "H001");
    assert_eq!(after, before);
}

#[test]
fn card_restore_reverse_detent_moves_toward_tail() {
    let mut leaf = card_restore_fixture();
    card_restore_widen(&mut leaf);
    let before = card_restore_first(&leaf);
    let mut carry = None;
    let steps = CardAim::spend(
        &mut carry,
        LeafId {
            tab: TabId(1),
            seat: SeatId(1),
        },
        MouseScrollDelta::LineDelta(0.0, -1.0),
    );
    aim_card_window(&mut leaf, 4, steps, card_trace::Card::untraced());
    let after = card_restore_first(&leaf);
    eprintln!("reverse projection: {before} -> {after}");
    assert_eq!(after, "H002");
    assert_ne!(after, before);
}

/// A deferred resize is persisted as the number the reader chose, and drawn
/// as the number the pane can reach (T-CARD-NO-PASSIVE-CLAMP).
#[test]
fn card_restore_deferred_resize_keeps_numeric_persistence() {
    let mut leaf = card_restore_fixture();
    card_restore_resize(&mut leaf, 40, 40, LeafOnStage::Behind);
    assert_eq!(leaf.card_skip, 130);
    card_restore_settle(&mut leaf);
    assert_eq!(card_restore_first(&leaf), "H001");
    // The session file carries the raw number, saturating at `u32`, and a
    // restart hands back what it carried.
    let saved = u32::try_from(leaf.card_skip).unwrap_or(u32::MAX);
    leaf.card_skip = saved as usize;
    assert_eq!(card_restore_first(&leaf), "H001");
    assert_eq!(leaf.card_skip, 130);
    // The reflow left nothing for the hand to pay off either.
    aim_card_window(&mut leaf, 4, -1, card_trace::Card::untraced());
    assert_eq!(leaf.card_skip, 115);
    assert_eq!(card_restore_first(&leaf), "H002");
}

#[test]
fn card_restore_boundary_discards_overflow_before_reversal() {
    let mut leaf = card_restore_fixture();
    card_restore_widen(&mut leaf);
    aim_card_window(&mut leaf, 4, i32::MAX, card_trace::Card::untraced());
    assert_eq!(card_restore_first(&leaf), "H001");
    aim_card_window(&mut leaf, 4, i32::MAX, card_trace::Card::untraced());
    let mut carry = None;
    let steps = CardAim::spend(
        &mut carry,
        LeafId {
            tab: TabId(1),
            seat: SeatId(1),
        },
        MouseScrollDelta::LineDelta(0.0, -1.0),
    );
    aim_card_window(&mut leaf, 4, steps, card_trace::Card::untraced());
    assert_eq!(card_restore_first(&leaf), "H002");
}

/// RED (0.4.4 ticket 02) — **while the card is up, a key other than Enter, Tab, Shift+Tab or Esc
/// reaches nothing and the card stays** (owner's rulings 2026-09-23; `Shift+Tab` joined the keys
/// with the two-button-dialog ruling of the same day).
///
/// The key's meaning is `paste_card_key`; that it reaches nothing is the rung: it stands above
/// every road to a shell in `keyboard_input` and returns whatever the key was.
///
/// MUTATION: map `Key::Character` to `Cancel` in `paste_card_key`, or take the `return Ok(())`
/// out of the card's rung.
#[test]
fn a_key_other_than_enter_tab_or_esc_reaches_nothing_and_leaves_the_card_up() {
    use winit::keyboard::ModifiersState;
    let none = ModifiersState::empty();
    for key in [
        Key::Character("a".into()),
        Key::Character("v".into()),
        Key::Named(NamedKey::Space),
        Key::Named(NamedKey::Backspace),
        Key::Named(NamedKey::ArrowUp),
        Key::Named(NamedKey::F5),
    ] {
        assert_eq!(
            paste_card_key(&key, none, PasteAnswer::RunLineByLine),
            None,
            "{key:?}"
        );
    }
    for (key, modifiers) in [
        (Key::Character("v".into()), ModifiersState::CONTROL),
        (Key::Named(NamedKey::Enter), ModifiersState::SHIFT),
        (Key::Named(NamedKey::Enter), ModifiersState::CONTROL),
        (Key::Named(NamedKey::Tab), ModifiersState::CONTROL),
        (Key::Named(NamedKey::Tab), ModifiersState::ALT),
        (Key::Named(NamedKey::Escape), ModifiersState::ALT),
    ] {
        assert_eq!(
            paste_card_key(&key, modifiers, PasteAnswer::RunLineByLine),
            None,
            "{key:?} {modifiers:?}"
        );
    }
    // The card is still up after them: nothing took the paste.
    let (mut tab, target) = paste_tab(paste_leaf(shell_literal::ShellGrammar::Cmd, b""));
    assert_eq!(
        paste_text_into(&mut tab, target, THREE_LINES, true),
        StagedPaste::Held
    );
    assert!(pending_paste_in(&tab).is_some());

    let ladder = squeezed_body("Runtime", "keyboard_input");
    let rung = "ifself.paste_card_seat().is_some(){if!event.repeat&&letSome(focus)=self.paste_card_focus()&&letSome(key)=paste_card_key(&event.logical_key,self.window.modifiers,focus){self.press_paste_card_key(key)?;}returnOk(());}";
    let at = ladder
        .find(rung)
        .unwrap_or_else(|| panic!("the card's rung is not whole"));
    for road in [
        "self.paste_from_clipboard()?;",
        "self.copy_selection()?;",
        "send_user_input(",
    ] {
        if let Some(later) = ladder.find(road) {
            assert!(at < later, "`{road}` is reached before the card's rung");
        }
    }
}
