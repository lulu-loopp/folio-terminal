//! **`seats`, as the application drives it.** Tests whose first assertion is about
//! `seats`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    CROSS_DPI, SHOT_PATH, TAB_ONE, answer, breathing, buffer_saying, cross_merge, cross_metrics,
    cross_move, cross_move_at, cross_seats, cross_solve, cross_tab, dir_entry, document_on,
    edited_buffer, files_column, grid_of, leaf_saying, leaf_says, listed, listing, logical_width,
    one_turn, pane_box_of, pane_rects_of, pictures_drawn, presentation_of, request_attention,
    restored_three_terminals_before_the_window_is_maximized, restored_two_previews_and_a_terminal,
    ring, ringing_tab, ruler, seat_of, settled, solved_lopsided_split, split_window, squeezed_body,
    tab_texts, tab_with_a_files_column, tab_with_a_picture, tab_with_a_preview,
};
use bt_render::LIGHT_CHROME;
use std::time::Duration;

/// PIN (D4) — **a press whose row went with the editor is spent on closing
/// the editor, and reaches nothing else.**
///
/// The other half of the rule. The pending row is the clear case — it exists
/// only while the box is open — and a rename that re-keys the row it was
/// typed in is the same shape: the identity the press named is not in the
/// rebuilt list, and no other row inherits it.
///
/// RED GATE: answer `AskAgain` for a missing row and the press falls through
/// to whatever the rebuilt list put under the pointer.
#[test]
fn a_press_whose_row_is_gone_is_consumed() {
    let (mut tab, seat) = files_column(r"D:\work");
    tab.file_trees
        .entry(seat)
        .or_default()
        .accept("", listed(vec![dir_entry("a.txt", false)]));
    let one = |tab: &TabState, place| -> BTreeMap<SeatId, seats::FilesTreeContent> {
        tab.files_tree_walk(place)
            .into_iter()
            .map(|(seat, (content, _))| (seat, content))
            .collect()
    };
    let measured = one(
        &tab,
        Some(FilesEditPlace {
            seat,
            at: FilesEditRow::New {
                parent: "",
                folder: false,
            },
        }),
    );
    let rebuilt = one(&tab, None);

    let pending = pressed_row_identity(
        Some(seats::ChromeTarget::FilesRow { seat, index: 1 }),
        &measured,
    );
    assert_eq!(
        pending,
        Some((seat, "/".to_owned())),
        "the pending row's own key, which no entry on a disk can have"
    );
    assert_eq!(press_after_blur(pending, &rebuilt), PressAfterBlur::Gone);

    // And a row whose name changed under the commit is gone by the same rule.
    assert_eq!(
        press_after_blur(Some((seat, "/old.txt".to_owned())), &rebuilt),
        PressAfterBlur::Gone
    );
    assert_eq!(
        press_after_blur(Some((seat, "/a.txt".to_owned())), &rebuilt),
        PressAfterBlur::Row(seat, 0),
        "while a row that is still there is still the row that was pressed"
    );
}

/// **A tab with two preview panes, out to disk and back** — N44, the whole
/// of slice 7's ruling in one pass.
///
/// The pin rides in the tree, the file and the pool ride in the content
/// section beside it, and the section names each pane by the same
/// positional token `focused_leaf` uses. What this gate is really for is the
/// *pairing*: a section that paired by position in its own list would put
/// the pinned pane's file on the unpinned pane the moment the two lists
/// disagree, which looks like a working preview showing the wrong document.
///
/// MUTATION ①: have `PreviewRestore::from_persisted` zip `saved.panes`
/// against `seats.preview_seats()` instead of resolving the token — the
/// second assertion goes red, because the fixture deliberately writes the
/// two panes in the other order.
/// MUTATION ②: drop the `SeatKind::Preview` check in `from_persisted` — the
/// terminal row lands a file on the shell's seat and the last assertion
/// goes red.
#[test]
fn two_preview_panes_come_back_on_the_files_the_section_named() {
    let metrics = cross_metrics();
    let mut seats = seats::Seats::lone_terminal();
    let pinned = seats
        .add_preview(&metrics)
        .expect("the first preview lands");
    assert!(seats.toggle_preview_lock(pinned));
    let landing = seats
        .add_preview(&metrics)
        .expect("a pinned pane is not a reuse target, so a second lands beside it");
    assert_ne!(pinned, landing, "the pin bought a second pane");

    let token = |seat: SeatId| {
        let index = seats
            .tree()
            .seats_in_order()
            .iter()
            .position(|found| found.id == seat)
            .expect("the seat is in the tree it came from");
        format!("leaf-{index}")
    };
    let readme = PathBuf::from(r"C:\repo\README.md");
    let saved = bt_persist::TabPreviewV1 {
        // Deliberately in the *other* order from `preview_seats()`, so a
        // positional pairing cannot pass by accident.
        panes: vec![
            bt_persist::PreviewPaneV1 {
                leaf: token(landing),
                cur: None,
                cur_source: bt_persist::PreviewSourceV1::File,
                graph: None,
            },
            bt_persist::PreviewPaneV1 {
                leaf: token(pinned),
                cur: Some(readme.to_string_lossy().into_owned()),
                cur_source: bt_persist::PreviewSourceV1::File,
                graph: None,
            },
            // A row naming the terminal: a hand edit, or a tree a newer
            // build shaped differently. It costs that row and nothing else.
            bt_persist::PreviewPaneV1 {
                leaf: token(seats.identity()),
                cur: Some(r"C:\repo\stray.md".to_owned()),
                cur_source: bt_persist::PreviewSourceV1::File,
                graph: None,
            },
        ],
        pool: vec![
            bt_persist::PreviewPoolEntryV1 {
                path: readme.to_string_lossy().into_owned(),
                name: "README.md".to_owned(),
                source: bt_persist::PreviewSourceV1::File,
            },
            bt_persist::PreviewPoolEntryV1 {
                path: r"C:\repo\notes.md".to_owned(),
                name: "notes.md".to_owned(),
                source: bt_persist::PreviewSourceV1::File,
            },
        ],
    };

    let restored = PreviewRestore::from_persisted(&seats, Some(&saved));
    assert_eq!(
        restored.cur.get(&pinned),
        Some(&preview::PreviewSource::file(readme.clone())),
        "the file lands on the leaf the section named"
    );
    assert_eq!(
        restored.cur.get(&landing),
        None,
        "a pane that was showing nothing comes back showing nothing"
    );
    assert!(
        !restored.cur.contains_key(&seats.identity()),
        "a row naming a leaf of another kind is dropped, not obeyed"
    );
    assert_eq!(
        restored.pool.len(),
        2,
        "the pool is a history, not a screen"
    );

    // And the write side agrees with the read side, which is the only way
    // the round trip can be a round trip. Assembled the way every other tab
    // is, so nothing here is a second opinion about what a tab is.
    let sessions = BTreeMap::from([(seats.identity(), leaf_saying("SHELL"))]);
    let focused = seats.identity();
    let mut pool = preview::PreviewPool::default();
    for (source, name) in &restored.pool {
        pool.insert(preview::PreviewBuffer::new(source.clone(), name.clone()));
    }
    let mut panes = PreviewPanes::default();
    panes.entry(seat_of(TAB_ONE, pinned)).buffer =
        Some(preview::PreviewSource::file(readme.clone()));
    let (layout, overflow) = cross_solve(&seats);
    let tab = assemble_tab_state(
        TabId(1),
        sessions,
        BTreeMap::new(),
        pool,
        panes,
        BTreeMap::new(),
        focused,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    );

    let written = tab.preview_content().expect("this tab has previews");
    assert_eq!(
        written.panes.len(),
        2,
        "one row per preview leaf, in tree order"
    );
    let round_tripped = PreviewRestore::from_persisted(&tab.seats, Some(&written));
    assert_eq!(
        round_tripped.cur, restored.cur,
        "what was written is what comes back on the same seats"
    );
    assert_eq!(
        tab.preview_pages(),
        vec![bt_persist::RecentPreviewV1::File(
            readme.to_string_lossy().into_owned()
        )],
        "and the vault takes the files, skipping the pane that had none (裁决 10)"
    );
}

/// One pane's column count, from the rectangle it is sized from.
///
/// The solve is stood in for the way
/// [`minimizing_a_window_never_tells_its_shell_the_width_of_the_icon`] stands
/// it in — a cell of the shipped face at 200% is 19 physical pixels wide —
/// and `CellMetrics::grid_for_pixels`' own floor is applied, because the
/// floor is where the two columns in the report came from.
fn columns_of(width: u32) -> u32 {
    (width / 19).clamp(
        u32::from(bt_render::CellMetrics::MIN_COLUMNS),
        u32::from(u16::MAX),
    )
}

/// **PIN (user report, 2026-09-09) — a pane is never born at the width of a
/// bar.**
///
/// The report is a restored window in focus mode whose tab held three
/// PowerShell panes: the third pane's first prompt read `(b` on one line and
/// `ase) PS D:\Documents\SyncFolder\Application> ` on the next, and that
/// tab's card drew the seat as rows of two characters. `session.json` names
/// the cause — the window is saved `maximized: true` beside a *normal*
/// rectangle of 960x600, and `put_the_window_on_the_glass` asks Windows to
/// maximize it only after every shell has been spawned. So the tabs are
/// built against 960 logical pixels, focus mode takes 280 of them for the
/// card column, and L3 buys the missing room exactly as it is supposed to:
/// the seat farthest from the focus stops being a pane and becomes a
/// [`bt_layout::COLLAPSED_EXTENT`] bar.
///
/// **A bar is a presentation, and 24 logical pixels is not a size a shell may
/// be told about.** At 200% it is 48 physical pixels, which is
/// `CellMetrics::MIN_COLUMNS`, which is two — so the ConPTY was spawned two
/// columns wide and the shell printed its prompt there. A reflow is not an
/// undo (the same sentence
/// [`minimizing_a_window_never_tells_its_shell_the_width_of_the_icon`]
/// writes): every row that scrolled off at that width stays two characters
/// to the row for the rest of the pane's life.
///
/// Red gate: size the shell from [`seats::pane_body_viewport`], which is what
/// `create_tab_state` did, and the bar's own 48 pixels come through as two
/// columns.
#[test]
fn a_pane_is_never_born_at_the_width_of_a_bar() {
    let (seats, metrics, layout) = restored_three_terminals_before_the_window_is_maximized();
    // The ladder did what the ladder is for, and this test is not an argument
    // against it: one of the three is a bar.
    let bar = seats
        .terminals()
        .into_iter()
        .find(|seat| presentation_of(&layout, *seat).is_collapsed_along(bt_layout::Axis::Row))
        .expect("the window is too narrow for three panes, so L3 folded one");
    // What that bar is worth as a terminal, and it is the number in the
    // report.
    let folded = seats::pane_body_viewport(&seats, &layout, bar, 2.0)
        .expect("a collapsed seat still holds its place in the tree");
    assert_eq!(
        columns_of(folded.width),
        u32::from(bt_render::CellMetrics::MIN_COLUMNS),
        "the bar really is worth two columns — this is the rectangle the report came off"
    );
    // And what the shell is actually born into.
    let born = seats::birth_body_viewport(&seats, &layout, bar, &metrics, 2.0);
    assert!(
        columns_of(born.width) > u32::from(bt_render::CellMetrics::MIN_COLUMNS),
        "a shell was spawned at the width of a bar: {} columns",
        columns_of(born.width)
    );
    // Not merely "more than two" — the floor is the solver's own minimum for
    // a terminal, which is the smallest rectangle this product ever shows one
    // in.
    let floor = bt_layout::MIN_PANE_W.floor_px() as u32 * 2;
    assert!(
        born.width >= floor,
        "born {} physical pixels wide, and a terminal pane is never narrower than {floor}",
        born.width
    );
    // The two panes the ladder left alone are untouched: this rule is about
    // seats that are not being shown as panes, and about nothing else.
    for seat in seats.terminals().into_iter().filter(|seat| *seat != bar) {
        assert_eq!(
            seats::birth_body_viewport(&seats, &layout, seat, &metrics, 2.0).width,
            seats::pane_body_viewport(&seats, &layout, seat, 2.0)
                .expect("a pane was placed")
                .width,
            "a pane's own rectangle is the one it is born into"
        );
    }
}

/// **A window too narrow for its restored tree degrades explicitly, and the
/// terminal is the last thing it gives up** (§2.6.1 L3, 用户裁决
/// 2026-08-13).
///
/// Two preview panes and a terminal want 980 logical pixels of floor plus two
/// dividers, and a restored 960-wide window has not got it. Two things are
/// pinned here and they are separate. *What* a fold is:
/// `Presentation::Collapsed`, the ruled §2.6.3 degradation (a clickable title
/// bar, still in the tree, still focusable) at exactly `COLLAPSED_EXTENT` —
/// not a seat handed some arbitrary width below its own minimum. And *who*
/// folds: the case 7 capture had the **terminal** reduced to a strip, because
/// distance alone chose and the focus happened to sit on a preview at the far
/// end. The class now leads the distance, so what gives way is a preview —
/// a quick look at a file that is still on disk — and the pane running
/// somebody's shell keeps its rectangle.
///
/// Red gate: give a seat a width between `COLLAPSED_EXTENT` and its own
/// minimum and the `Collapsed` assertion fails — which is the difference
/// between an honest fold and a crush. Take `collapse_rank` out of
/// `collapse_order` and the terminal is the one wearing the bar again.
#[test]
fn a_restore_too_narrow_for_its_tree_folds_a_preview_and_never_the_terminal() {
    let (seats, metrics, viewport) = restored_two_previews_and_a_terminal();
    let terminal = seats.identity();
    let focus = seats.focus();
    let layout = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("the concession chain has an answer for this window");
    assert_eq!(
        presentation_of(&layout, terminal),
        bt_layout::Presentation::Full,
        "the terminal is the last non-focus seat to fall, and it did not have to"
    );
    assert!(
        logical_width(&layout, terminal) >= bt_layout::MIN_PANE_W.floor_px(),
        "and it kept a whole pane's width: {}",
        logical_width(&layout, terminal)
    );

    let folded: Vec<SeatId> = seats
        .preview_seats()
        .into_iter()
        .filter(|seat| {
            presentation_of(&layout, *seat)
                == bt_layout::Presentation::Collapsed(bt_layout::AxisSet::ROW)
        })
        .collect();
    assert_eq!(folded.len(), 1, "one fold was enough to pay for the window");
    assert_ne!(
        folded[0], focus,
        "W2: the focus seat is never the one folded"
    );
    assert_eq!(
        logical_width(&layout, folded[0]),
        bt_layout::COLLAPSED_EXTENT.floor_px(),
        "and a fold is exactly the ruled extent, not whatever was left over"
    );
    assert!(
        logical_width(&layout, focus) >= bt_layout::MIN_PREVIEW_W.floor_px(),
        "what the fold bought is the pane you were reading, at or above its floor"
    );
}

/// PIN (user ruling, 2026-08-15): **every fact that shapes a wheel notch's
/// bytes is read from the pane the notch is over.**
///
/// [`wheel_route`] having lost its focus parameter settles the *decision*;
/// this settles the *facts fed to it*, which is where the same bug would
/// grow back and would grow back silently. The shipped handler asked the
/// window for `self.session.terminal_modes()`, `self.session.application_
/// cursor_mode()` and `self.grid.rows` — every one of them a
/// [`Deref`] into whichever leaf holds the keyboard. Route the hovered
/// pane by its own modes and then encode with the focused pane's cursor
/// mode and a hovered vim gets `ESC [ A` where it wanted `ESC O A`: not a
/// scroll but the letters `[A` typed into a buffer.
///
/// Two panes, deliberately opposite in all three facts, and the keyboard
/// deliberately in the *plain* one — which is exactly the user's report:
/// pointer over the TUI, focus in the shell next door.
///
/// MUTATION: implement any of the three readers as `self.session.…` or
/// `self.grid.…` — the shipped shape — and the matching assertion goes red,
/// while the control assertions at the end keep proving the fixture really
/// did differ.
#[test]
fn a_wheel_reads_the_hovered_panes_own_shell_and_not_the_keyboards() {
    use bt_term::MouseTracking;
    // Pane 0 is a plain shell and holds the keyboard; pane 1 is a
    // full-screen program that asked for mouse reports and application
    // cursor keys — a TUI, which is what the user was hovering.
    let mut tab = cross_tab(1, &["plain shell", "\x1b[?1049h\x1b[?1002h\x1b[?1h"]);
    let [keyboard, hovered] = tab.seats.terminals()[..] else {
        panic!("a two-pane cross tab holds two terminal seats");
    };
    assert_eq!(
        tab.focused_leaf, keyboard,
        "the fixture only means anything with the keyboard in the other pane"
    );
    // Panes of a real split are rarely the same height; give them different
    // row counts so "one screen at a time" has two possible answers.
    tab.sessions
        .get_mut(&hovered)
        .expect("the hovered seat holds a session")
        .grid
        .rows = std::num::NonZeroU16::new(17).expect("a nonzero row count");

    let modes = tab.leaf_terminal_modes(hovered);
    assert!(
        modes.alternate_screen && modes.mouse_tracking != MouseTracking::Off,
        "the hovered pane's own modes: a full-screen program asking for reports, {modes:?}"
    );
    assert!(
        tab.leaf_application_cursor_mode(hovered),
        "and its own cursor-key mode, which decides ESC O A against ESC [ A"
    );
    assert_eq!(
        tab.leaf_wheel_rows(hovered),
        17.0,
        "and its own row count, which is what a page of it means"
    );

    // The control: the keyboard's pane really is the opposite in all three,
    // so none of the three assertions above could have passed by reading it.
    let keyboard_modes = tab.leaf_terminal_modes(keyboard);
    assert!(
        !keyboard_modes.alternate_screen && keyboard_modes.mouse_tracking == MouseTracking::Off,
        "the keyboard's pane is a plain shell, {keyboard_modes:?}"
    );
    assert!(!tab.leaf_application_cursor_mode(keyboard));
    assert_eq!(tab.leaf_wheel_rows(keyboard), 4.0);
    assert_eq!(
        tab.shell().session.terminal_modes(),
        keyboard_modes,
        "and the deref really does answer with the keyboard's pane — which is \
             the whole reason the readers above must not use it"
    );
}

/// PIN (user report, 2026-08-20) — **a full-screen program that holds the
/// grid all day does not make the tab breathe all day.**
///
/// §7.1.5c's `alt-screen 一律不记` reaching the busy channel (the ruling that
/// takes it there is the present-tense paragraph at the end of §7.1.5b). The breath's
/// verdict is unchanged and stays the one UI-UX §8 wrote down — "运行中 =
/// OSC 133 C 来了、D 还没来" — but a long-lived TUI is one command by that
/// reading for its entire life, so on the alternate screen the sentence is
/// true and carries nothing. The report: `claude` started from a pwsh
/// prompt breathed for a whole day and asked the event loop for a frame the
/// whole time.
///
/// The exemption is the *breath's* alone. The ring (OSC 9;4) and the dot
/// (BEL, unread) are channels a program on the alternate screen still
/// reaches on purpose — a full-screen installer reporting progress, an agent
/// ringing for an answer — and this must not close them, so they are asked
/// for here beside it.
///
/// Red gate: leave [`TabState::fleet_working`] reading `working` alone and
/// the tab breathes with the TUI on screen, and
/// [`TabState::mark_is_animating`] goes on owing a frame every 16ms for a
/// picture that never changes.
#[test]
fn the_breath_stands_down_on_the_alternate_screen() {
    let mut tab = cross_tab(1, &["SHELL"]);
    let seat = tab.seats.terminals()[0];
    let mid_breath =
        tab.animation_epoch + Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS).mul_f32(0.5);
    let palette = bt_render::chrome_palette();

    let feed = |tab: &mut TabState, bytes: &[u8]| {
        tab.sessions
            .get_mut(&seat)
            .expect("the tab's one shell")
            .session
            .feed(bytes)
            .expect("the fixture's bytes parse");
    };

    // The shell says a command started, on the primary screen, where it did.
    feed(
        &mut tab,
        b"\x1b]133;A\x07PS> \x1b]133;B\x07claude\x1b]133;C\x07",
    );
    assert!(tab.fleet_working(), "a command is running");
    assert!(tab.mark_is_animating(mid_breath, Motion::Full));
    assert!(
        tab.mark_state(true, mid_breath, Motion::Full, &palette)
            .opacity
            < 1.0,
        "and the mark is drawn breathing"
    );

    // That command is a full-screen program, and it takes the grid.
    feed(&mut tab, b"\x1b[?1049h");
    assert!(
        !tab.fleet_working(),
        "a TUI holding the screen is not a shell with work in flight"
    );
    assert!(
        !tab.mark_is_animating(mid_breath, Motion::Full),
        "and nothing in the slot is moving, so no frame is owed for it"
    );
    assert_eq!(
        tab.mark_state(true, mid_breath, Motion::Full, &palette)
            .opacity,
        1.0,
        "the mark is simply itself"
    );

    // The two channels that stay open across the exemption, on purpose.
    feed(&mut tab, b"\x1b]9;4;3\x07");
    assert!(
        tab.mark_is_animating(mid_breath, Motion::Full),
        "an indeterminate ring reported from the alternate screen still spins"
    );
    assert_eq!(
        tab.fleet_progress(),
        Some(ProgressState::Indeterminate),
        "the ring is a separate channel and the exemption does not touch it"
    );
    feed(&mut tab, b"\x1b]9;4;0\x07\x07");
    assert!(
        tab.sessions[&seat].session.status().bell_latched(),
        "and a bell rung from the alternate screen is still a bell"
    );

    // The program exits and hands the grid back. The shell's command never
    // ended, so the breath comes back with it.
    feed(&mut tab, b"\x1b[?1049l");
    assert!(
        tab.fleet_working(),
        "back on the primary screen the shell's own C is authoritative again"
    );
    assert!(tab.mark_is_animating(mid_breath, Motion::Full));

    feed(&mut tab, b"\x1b]133;D;0\x07");
    assert!(!tab.fleet_working(), "and the shell's D ends it");
}

/// The dual of the pin above: nothing changes for a command that spends its
/// whole life on the primary screen, which is every ordinary build.
///
/// Written out because the exemption is a new term in a predicate that had
/// none, and a term that answered `false` too often would stop the breath on
/// `cargo build` — the one thing it exists for.
#[test]
fn a_command_on_the_primary_screen_breathes_from_c_to_d() {
    let mut tab = cross_tab(2, &["SHELL"]);
    let seat = tab.seats.terminals()[0];
    let mid_breath =
        tab.animation_epoch + Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS).mul_f32(0.5);
    let palette = bt_render::chrome_palette();

    tab.sessions
        .get_mut(&seat)
        .expect("the tab's one shell")
        .session
        .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07cargo build\x1b]133;C\x07")
        .expect("the fixture's bytes parse");
    assert!(tab.fleet_working());
    assert!(!tab.sessions[&seat].session.status().alternate_screen);

    // Output for a while — the breath is the command's, not the bytes'.
    for _ in 0..8 {
        tab.sessions
            .get_mut(&seat)
            .expect("the tab's one shell")
            .session
            .feed(b"   Compiling bt-app v0.0.0\r\n")
            .expect("the fixture's bytes parse");
        assert!(tab.fleet_working(), "still between C and D");
        assert!(tab.mark_is_animating(mid_breath, Motion::Full));
        assert!(
            tab.mark_state(true, mid_breath, Motion::Full, &palette)
                .opacity
                < 1.0
        );
    }

    tab.sessions
        .get_mut(&seat)
        .expect("the tab's one shell")
        .session
        .feed(b"\x1b]133;D;0\x07")
        .expect("the fixture's bytes parse");
    assert!(!tab.fleet_working(), "D ends it, as it always did");
    assert_eq!(
        tab.mark_state(true, mid_breath, Motion::Full, &palette)
            .opacity,
        1.0
    );
}

/// PIN (T2, real-machine bug — the symmetric paths): every other channel
/// that can *stop* is owed the same final frame.
///
/// The root cause was never specific to the breath: it was a scheduler that
/// could not see a channel switching off. These are the three siblings that
/// were broken by the same line, and each is checked here so a future
/// "optimisation" back to an is-it-moving test fails loudly rather than
/// silently freezing one channel at a time.
#[test]
fn every_channel_that_stops_is_owed_its_final_frame() {
    let palette = LIGHT_CHROME;

    // 1. An indeterminate ring clearing back to its mark. This one was
    //    doubly hidden: the old ring signal watched the sweep and the
    //    tween, and an indeterminate ring keeps neither.
    let spinning = seats::TabMarkState {
        ring: Some(seats::TabRing {
            arc: palette.accent,
            start_milliturns: 250,
            sweep_milliturns: 243,
        }),
        ..seats::TabMarkState::default()
    };
    let cleared = seats::TabMarkState::default();
    assert!(
        tab_owes_frame(Some(spinning), cleared),
        "a ring that clears must hand the slot back to the mark"
    );

    // 2. Reduced motion, where the mark never animates at all — it steps
    //    between two held values. An is-it-moving test is blind to *both*
    //    edges here, so the held .6 would never arrive and never leave.
    let held = breathing(Motion::Reduced);
    assert_eq!(held.opacity, WINDOW_TAB_BREATHE_REDUCED_OPACITY);
    let done = settled(Motion::Reduced);
    assert!(
        tab_owes_frame(Some(done), held),
        "reduced motion must still show that work has started"
    );
    assert!(tab_owes_frame(Some(held), done), "and that it has finished");

    // 3. A dot arriving on a tab that is not moving and never was — a bell
    //    on a background tab. Nothing animates, so nothing asked to draw.
    let quiet_tab = seats::TabMarkState::default();
    let ringing = seats::TabMarkState {
        dot: StatusClaim::Bell.dot(&palette),
        ..seats::TabMarkState::default()
    };
    assert!(
        tab_owes_frame(Some(quiet_tab), ringing),
        "a bell on a still tab must still light its dot"
    );
}

/// PIN (§7.1.5b; attention block 2026-08-25) — **one tab, one frame, one
/// sample: the card's edge, its halo and every surface's dot read the same
/// reading of the same clock.**
///
/// *"所以点、脉动与卡片橙框读的就是这个答案"*. The sample is taken once,
/// in [`TabState::mark_state`], off the tab's own `animation_elapsed`, and
/// travels as [`seats::TabMarkState::pulse`]. This asserts that what arrives
/// there **is** `wait_pulse`'s reading at that instant — not a number of the
/// same shape arrived at somewhere else.
///
/// Red gate: sample either face anywhere but `mark_state` and the equality
/// fails the moment the two clocks differ by a frame; key the pulse on anything
/// but `StatusClaim::pulses` and the bell's line goes red.
#[test]
fn a_waiting_tabs_pulse_is_one_reading_of_one_clock() {
    let palette = bt_render::chrome_palette();
    let period = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS);
    let mut tabs = vec![ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let mut next = attention::Places::default();
    let elapsed = period.mul_f32(0.25);
    let quarter = tabs[0].animation_epoch + elapsed;

    // Nothing is standing in the queue yet, so there is no sample at all.
    assert_eq!(
        tabs[0]
            .mark_state(false, quarter, Motion::Full, &palette)
            .pulse,
        None,
        "a tab with nobody waiting on it has no breath to hand down"
    );

    // A pane asks for an answer and the pass gives it its place in the queue.
    request_attention(&mut tabs[0], seat, "yes");
    one_turn(&mut tabs, 1, false, &mut next);
    assert!(
        ticket_at(&tabs[0], seat).is_some(),
        "the fixture's own precondition: this pane really is in the queue"
    );

    let state = tabs[0].mark_state(false, quarter, Motion::Full, &palette);
    assert_eq!(
        state.pulse,
        Some(wait_pulse(elapsed, Motion::Full)),
        "and what it hands down is `wait_pulse`'s own reading at that instant"
    );
    assert!(
        state.dot.is_some_and(|dot| !dot.hollow),
        "beside the filled warn dot the very same claim answers for"
    );

    // **And the loop is woken for it, whatever the tab layout is.**
    // `mark_is_animating` is a fact about the tab and `strip_animation_work`
    // folds it over every tab with no layout branch anywhere in the walk —
    // which is what carries the frames to the horizontal strip and to the
    // rail's rows, the two surfaces that wear no halo and whose dot is
    // therefore the only thing on them that moves.
    assert!(
        tabs[0].mark_is_animating(quarter, Motion::Full),
        "a queue place that stands owes the next frame"
    );
    assert!(
        !tabs[0].mark_is_animating(quarter, Motion::Reduced),
        "and none at all with the system's animations off, where nothing moves"
    );

    // The reader types into the pane: the place is served through the out door
    // the window itself uses, and everything about it goes quiet again. A dot
    // that had gone on asking for a frame every 16ms after the thing it was
    // about ended is the wake-up budget spent on a still picture.
    answer(&mut tabs, 0, seat, UserInputKind::Keyboard, &mut next, None);
    assert_eq!(
        ticket_at(&tabs[0], seat),
        None,
        "the fixture's second precondition: answering it takes the place back"
    );
    assert_eq!(
        tabs[0]
            .mark_state(false, quarter, Motion::Full, &palette)
            .pulse,
        None,
        "no claim, no sample"
    );
    assert!(
        !tabs[0].mark_is_animating(quarter, Motion::Full),
        "and nothing in the slot is moving, so no frame is owed for it"
    );
}

fn ticket_at(tab: &TabState, seat: SeatId) -> Option<u64> {
    tab.sessions[&seat].attention.ticket()
}

/// PIN (`attention` plan §4 B2, user ruling 2026-08-25) — **a bell takes no place, anywhere.**
///
/// The retirement, as a truth table, because "we removed it" is a claim about four rows and not
/// about one. A `BEL` is what a program sends at the *end of a turn*; the badge it used to light
/// says "it is standing there waiting for you", and the four recordings of §2 are why those are
/// not the same sentence. What the bell keeps is the claim it always honestly had — a flat dot
/// that a look spends.
///
/// Red gate: put `bell_latched` back into the door and the first assertion goes red on three of
/// the four rows at once.
#[test]
fn a_bell_takes_no_place_wherever_it_rings() {
    for tab_is_active in [true, false] {
        for window_is_focused in [true, false] {
            let mut tabs = vec![ringing_tab(1, 1)];
            let seat = tabs[0].seats.terminals()[0];
            let mut next = attention::Places::default();
            ring(&mut tabs[0], seat);
            one_turn(
                &mut tabs,
                usize::from(!tab_is_active),
                window_is_focused,
                &mut next,
            );
            assert_eq!(
                ticket_at(&tabs[0], seat),
                None,
                "a bell on active={tab_is_active} focused={window_is_focused} asked for a place"
            );
            assert_eq!(next.issued(), 0, "and spent no serial");
            let watched = tab_is_active && window_is_focused;
            assert_eq!(
                tabs[0].sessions[&seat].session.status().bell_latched(),
                !watched,
                "the latch is spent by a look and by nothing else"
            );
            assert_eq!(
                tabs[0].fleet_claim_for(tab_is_active),
                if watched {
                    StatusClaim::Silent
                } else {
                    StatusClaim::Bell
                },
                "a bell claims `Bell`, which is the claim it can support"
            );
        }
    }
}

/// PIN (`attention` plan §11.1.4's `Settle` rows) — **a standing request behind a closed lid is
/// exactly what the queue is for, and one in front of you asks for nothing.**
///
/// The door into the queue, in full: it is [`attention_is_consumed`] read from the other side,
/// and every row of that `and` under a `not` is load-bearing. Only the pane that is *both* on
/// screen *and* in a focused window is refused — a background **window** consumes nothing, which
/// is the one moment a queue is doing its job.
#[test]
fn a_standing_request_asks_for_a_place_only_where_nobody_was_looking() {
    for tab_is_active in [true, false] {
        for window_is_focused in [true, false] {
            let mut tabs = vec![ringing_tab(1, 1)];
            let seat = tabs[0].seats.terminals()[0];
            let mut next = attention::Places::default();
            request_attention(&mut tabs[0], seat, "yes");
            one_turn(
                &mut tabs,
                usize::from(!tab_is_active),
                window_is_focused,
                &mut next,
            );

            let watched = tab_is_active && window_is_focused;
            assert_eq!(
                ticket_at(&tabs[0], seat).is_some(),
                !watched,
                "a request on active={tab_is_active} focused={window_is_focused}"
            );
            assert_eq!(
                tabs[0].fleet_claim_for(tab_is_active),
                if watched {
                    StatusClaim::Silent
                } else {
                    StatusClaim::Awaiting
                }
            );
            // 你正看着的终端不需要一枚徽章告诉你看它 — and it stays that way however many turns
            // run afterwards: nothing here is a race a later frame could still lose.
            for _ in 0..4 {
                one_turn(
                    &mut tabs,
                    usize::from(!tab_is_active),
                    window_is_focused,
                    &mut next,
                );
            }
            assert_eq!(ticket_at(&tabs[0], seat).is_some(), !watched);
            assert_eq!(
                next.issued(),
                u64::from(!watched),
                "and one serial at most was spent"
            );
        }
    }
}

/// PIN (§7.1.5b P1-8) — **a place is taken once and never re-stamped.**
///
/// 先到先服务 is a claim about *order*, and the way to lose it is to re-read the signal: a pane
/// that asks a second time while still unanswered would take a younger serial than one that
/// asked once and waited, which sorts the most insistent program last.
///
/// And the counter is never rewound, so two panes that ask on the same turn still have an order.
#[test]
fn a_place_in_the_queue_is_taken_once_and_never_re_stamped() {
    let mut tabs = vec![ringing_tab(1, 2), ringing_tab(2, 1)];
    let seats = tabs[0].seats.terminals();
    let (a, b) = (seats[0], seats[1]);
    let mut next = attention::Places::default();

    request_attention(&mut tabs[0], a, "yes");
    request_attention(&mut tabs[0], b, "yes");
    one_turn(&mut tabs, 1, true, &mut next);
    assert_eq!(
        (ticket_at(&tabs[0], a), ticket_at(&tabs[0], b)),
        (Some(0), Some(1)),
        "two panes asking on one turn still have an order"
    );
    assert_eq!(next.issued(), 2);

    // Restating it decides nothing — `bt-term` mints on the rise alone, so there is no edge to
    // read and no younger serial to hand out.
    for _ in 0..3 {
        request_attention(&mut tabs[0], a, "yes");
        one_turn(&mut tabs, 1, true, &mut next);
    }
    assert_eq!(
        ticket_at(&tabs[0], a),
        Some(0),
        "the loudest program must not sort last"
    );
    assert_eq!(next.issued(), 2, "and spends nothing");
}

/// PIN (§7.1.5b P1-8, verbatim) — **a place taken behind a closed lid survives the look,
/// survives the sibling's keystroke, and is retired only by an action in its own seat.**
///
/// Every clause of the out door in one walk. The widening of 2026-08-25 is in the last of them:
/// the key that answers is `1`, not `Enter`, because that is what answering a permission prompt
/// in Claude Code actually is.
#[test]
fn a_place_taken_behind_a_closed_tab_waits_for_an_action_in_its_own_seat() {
    let mut tabs = vec![ringing_tab(1, 1), ringing_tab(2, 3)];
    let seats = tabs[1].seats.terminals();
    let (a, b) = (seats[0], seats[1]);
    let mut next = attention::Places::default();

    // It asks while its tab is shut.
    request_attention(&mut tabs[1], b, "yes");
    one_turn(&mut tabs, 0, true, &mut next);
    assert_eq!(ticket_at(&tabs[1], b), Some(0));

    // The user switches to it and only looks.
    one_turn(&mut tabs, 1, true, &mut next);
    assert_eq!(
        ticket_at(&tabs[1], b),
        Some(0),
        "看一眼阻塞的 agent 不解除阻塞"
    );
    assert_eq!(tabs[1].fleet_claim_for(true), StatusClaim::Awaiting);

    // A keystroke in the sibling seat answers nothing: typing at the shell beside a blocked
    // agent does not unblock it either.
    answer(&mut tabs, 1, a, UserInputKind::Keyboard, &mut next, None);
    assert_eq!(ticket_at(&tabs[1], b), Some(0));

    // And a pointer sweeping across its own pane is not an answer — the one member of the
    // vocabulary that cannot be spelled as one (`attention` plan §11.3).
    answer(&mut tabs, 1, b, UserInputKind::MouseMotion, &mut next, None);
    assert_eq!(ticket_at(&tabs[1], b), Some(0));

    // A key in its own seat is what does it.
    answer(&mut tabs, 1, b, UserInputKind::Keyboard, &mut next, None);
    assert_eq!(ticket_at(&tabs[1], b), None);
    assert!(!tabs[1].fleet_awaiting());

    // And a later turn does not hand it back: the request is still standing, but it has been
    // answered, and `Acknowledged` is a fixed point under the pass. **This is the state a bit
    // could not represent**, and the whole reason the ledger counts generations.
    one_turn(&mut tabs, 1, true, &mut next);
    assert_eq!(ticket_at(&tabs[1], b), None);
    assert_eq!(next.issued(), 1, "no second serial was spent");
}

/// PIN (`attention` plan §11.1.3 rule 4) — **the program that withdraws takes the dot with it.**
///
/// The half a bell never had, and the reason `RequestAttention` is the sequence this is built
/// on: "it wants you" is a sentence that can be **taken back**. Nothing the user does is
/// involved, which is what makes it a second door rather than a second spelling of the first.
#[test]
fn a_withdrawn_request_gives_up_its_place_without_anybody_answering() {
    let mut tabs = vec![ringing_tab(1, 1), ringing_tab(2, 1)];
    let seat = tabs[1].seats.terminals()[0];
    let mut next = attention::Places::default();

    request_attention(&mut tabs[1], seat, "yes");
    one_turn(&mut tabs, 0, true, &mut next);
    assert_eq!(ticket_at(&tabs[1], seat), Some(0));

    request_attention(&mut tabs[1], seat, "no");
    one_turn(&mut tabs, 0, true, &mut next);
    assert_eq!(ticket_at(&tabs[1], seat), None);
    assert_eq!(tabs[1].fleet_claim_for(false), StatusClaim::Silent);

    // And asking again after a withdrawal is a **new** request, with a new place: the generation
    // it mints is above every watermark the last one was compared against.
    request_attention(&mut tabs[1], seat, "yes");
    one_turn(&mut tabs, 0, true, &mut next);
    assert_eq!(ticket_at(&tabs[1], seat), Some(1));
}

#[test]
fn repeated_tab_switches_do_not_feed_window_chrome_back_into_inner_size() {
    // The 2026-08-08 ruling closed this regression at the source: the minimum is the technical
    // floor, so it is the same number for every tab and there is no aggregate left to
    // alternate. The guard stays anyway — it is what keeps a *re-applied* minimum from being
    // handed to the frame at all, and winit's setter is still the setter it was.
    let metrics = seats::seat_metrics(1_000);
    let work_area = WorkAreaHint::Known(bt_layout::LogicalSize::px(2560, 1440));
    let floor = |seats: &seats::Seats| {
        let size = seats.min_inner_size(&metrics, work_area);
        (size.width.floor_px().max(1), size.height.floor_px().max(1))
    };
    let lone = seats::Seats::lone_terminal();
    let mut four = seats::Seats::lone_terminal();
    for _ in 0..3 {
        let target = four.identity();
        four.split_terminal(&metrics, target, Axis::Row, false)
            .expect("a terminal leaf splits");
    }
    assert_eq!(
        floor(&lone),
        floor(&four),
        "four columns do not get to decide how small the window may be"
    );

    let mut applied = None;
    assert!(window_minimum_changed(&mut applied, floor(&lone)));
    let mut mock_inner_size = PhysicalSize::new(960, 600);
    let stable_inner_size = mock_inner_size;

    for switch in 0..32 {
        let showing = if switch % 2 == 0 { &lone } else { &four };
        if window_minimum_changed(&mut applied, floor(showing)) {
            // Model the winit 0.30 Windows behavior that exposed the regression: every setter
            // call re-requests the current client size through non-client adjustment.
            mock_inner_size.height += 40;
        }
        assert_eq!(
            mock_inner_size, stable_inner_size,
            "tab switch {switch} changed the window inner size"
        );
    }
}

/// RED ③ — **an engine page in a tab with no shell reaches the glass**
/// (§7.10 ④‴, user report on `next21`: `.html` and `.pdf` came up blank).
///
/// A page is not drawn by this window at all — it is a DirectComposition
/// visual under the swapchain's, seen through a hole cut in this window's
/// own surface. Three things have to happen for one to be visible, and
/// **all three of them live on the present path**:
///
/// * `sync_web_page` tells the engine its rectangle and punches the hole,
/// * and it is reached only from `pane_draws`,
/// * and the composition tree is published only by `present_seats_and_commit`'s
///   `commit()` — wgpu's dx12 backend never commits a visual it did not
///   create the device for.
///
/// So for a tab whose only pane is a preview, "no present" and "no page" are
/// the same sentence: the engine sits at the size it was born with, behind a
/// surface that was never made transparent over it, and nothing on the glass
/// says so — which is a blank pane wearing a head that correctly names the
/// file.
///
/// The value half is the shape itself: such a tab really does answer "no
/// shell", which is what [`a_bare_redraw_still_owes_a_present`] is handed.
///
/// RED GATE: take `self.pane_draws(now)` out of `present_retained_picture`
/// — or the `sync_web_page` call out of `pane_draws` — and the chain breaks
/// at the assertion that names it. Either break is a page that is never
/// placed, which is the reported blank.
#[test]
fn an_engine_page_alone_in_a_tab_reaches_the_glass_through_the_retained_present() {
    // The shape: a tab whose only pane is a preview has no shell, which is
    // the bit the redraw door reads.
    let (seats, seat) = seats::Seats::lone_seat(&bt_layout::Seat::new(
        SeatId(1),
        bt_layout::SeatKind::Preview,
    ));
    let document = buffer_saying(r"D:\notes\report.html", "report.html", "<p>hi</p>");
    let source = document.source.clone();
    let mut pool = preview::PreviewPool::default();
    pool.insert(document);
    let mut panes = PreviewPanes::default();
    panes.entry(seat_of(TabId(9), seat)).buffer = Some(source);
    let (layout, overflow) = cross_solve(&seats);
    let tab = assemble_tab_state(
        TabId(9),
        BTreeMap::new(),
        BTreeMap::new(),
        pool,
        panes,
        BTreeMap::new(),
        seat,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    );
    assert!(
        tab.focused().is_none(),
        "a preview alone in a tab is a tab with nothing to type into, and \
             that is the fact the redraw door is handed"
    );
    assert!(
        a_bare_redraw_still_owes_a_present(false, tab.focused().is_some()),
        "so a redraw it asks for with no debt filed is still a present"
    );

    // The chain: the only path from that present to a page on the glass.
    let publisher = squeezed_body("Runtime", "publish_frame_inner");
    assert!(
            publisher.contains("ifself.focused().is_none(){self.window.chrome_present_pending=true;hang_watch::during(hang_watch::Station::WindowRedraw,||{self.window.window.request_redraw()});returnOk(false);}"),
            "a tab with no shell composes no terminal picture, and what it owes \
             instead is a present:\n{publisher}"
        );
    let retained = squeezed_body("Runtime", "present_retained_picture");
    assert!(
        retained.contains("letbodies=hang_watch::during(hang_watch::Station::RedrawLayout,||self.pane_draws(now));"),
        "the retained present is where such a tab's panes are placed:\n{retained}"
    );
    let draws = squeezed_body("Runtime", "pane_draws");
    assert!(
        draws.contains("self.sync_web_page(now);"),
        "and placing the panes is what tells the engine its rectangle and \
             cuts the hole it is seen through:\n{draws}"
    );
    let funnel = squeezed_body("Runtime", "present_seats_and_commit");
    assert!(
        funnel.contains(".commit(token)"),
        "and nothing else in this window publishes the composition tree the \
             page's visual lives in:\n{funnel}"
    );
}

/// PIN — the tab surface's tips come off the surface that is **drawn**.
///
/// Red gate, and it is a bug this caught rather than a shape being preserved:
/// `tab_strip_geometry` is a pure function of a width and a trailer list and
/// has no idea a rail is on screen, so under `Vertical` it went on handing
/// back a full run of tab, pin and mark boxes lying across the top bar — over
/// the sidebar toggle, which is the only thing actually drawn there. First
/// match wins in `TooltipAnchors` and those came first, so hovering the
/// visible toggle answered with tab 0's name.
///
/// The two halves are asserted together because either alone passes while
/// the other is broken: registering the rail's boxes without dropping the
/// strip's leaves both live, and dropping the strip's without the rail's
/// leaves the vertical layout with no tips at all.
#[test]
fn the_tab_surface_is_tipped_where_it_is_drawn_and_never_where_it_is_not() {
    let scale = 1.0;
    let width = 960.0;
    let trailers = [seats::TabTrailer {
        pinned: false,
        reveal: 1.0,
        ..seats::TabTrailer::default()
    }];
    let strip = seats::tab_strip_geometry(width, scale, crate::seats::FOLIO_BAR, &trailers, 0, 0.0);
    // The rail the window would actually be showing: vertical, expanded, and
    // fully open — `sampled_rail`'s own answer for a rail that is not the
    // parked icon kind.
    let rail = seats::rail_geometry(
        600.0,
        scale,
        seats::FOLIO_BAR,
        &trailers,
        0,
        0.0,
        seats::RailState {
            open: 1.0,
            ..rail_state_for(seats::TabLayoutMode::Vertical, seats::RailMode::Expanded)
        },
    )
    .expect("an expanded rail holding one tab is on screen");

    let flat = tab_surface_tip_boxes(
        seats::TabLayoutMode::Horizontal,
        &strip,
        Some(&rail),
        scale,
        false,
        None,
    );
    let railed = tab_surface_tip_boxes(
        seats::TabLayoutMode::Vertical,
        &strip,
        Some(&rail),
        scale,
        false,
        None,
    );

    // The same anchors on both surfaces — one `tabHtml` (mock-up 4333) and
    // one `paintStrip` (4366-4369) produce them, so neither layout is missing
    // a tip the other has.
    let ids = |boxes: &[(tooltip::TooltipAnchorId, [f32; 4])]| {
        boxes.iter().map(|(id, _)| *id).collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&flat),
        ids(&railed),
        "both surfaces tip the same things — the rail's `+` included (D32)"
    );
    assert!(
        ids(&flat).contains(&tooltip::TooltipAnchorId::NewTab)
            && ids(&flat).contains(&tooltip::TooltipAnchorId::NewTabMenu),
        "the `+` and its chevron are both tipped"
    );

    // And the boxes are the drawn surface's own. The rail is a column down
    // the left; the strip is a row across the top. A box from the wrong one
    // is the bug.
    let strip_boxes: Vec<_> = flat.iter().map(|(_, rect)| *rect).collect();
    let rail_boxes: Vec<_> = railed.iter().map(|(_, rect)| *rect).collect();
    assert!(
        strip_boxes.iter().all(|rect| rect[3] <= rail.body[1]),
        "every horizontal tip sits in the title bar band: {strip_boxes:?}"
    );
    assert!(
        rail_boxes
            .iter()
            .all(|rect| rect[1] >= rail.body[1] && rect[2] <= rail.body[2]),
        "every vertical tip sits inside the rail's own column: {rail_boxes:?}"
    );
    assert!(
        !rail_boxes.iter().any(|rect| strip_boxes.contains(rect)),
        "not one box survived from the surface that is not drawn"
    );

    // I94 holds on both: the chevron goes quiet under its own open menu.
    for layout in [
        seats::TabLayoutMode::Horizontal,
        seats::TabLayoutMode::Vertical,
    ] {
        let open = tab_surface_tip_boxes(layout, &strip, Some(&rail), scale, true, None);
        assert!(
            !ids(&open).contains(&tooltip::TooltipAnchorId::NewTabMenu),
            "{layout:?}: the chevron is silent while its own menu is up"
        );
        assert!(
            ids(&open).contains(&tooltip::TooltipAnchorId::NewTab),
            "{layout:?}: the `+` beside it still answers"
        );
    }

    // The editor IS the answer (mock-up 4193-4196), on both surfaces.
    for layout in [
        seats::TabLayoutMode::Horizontal,
        seats::TabLayoutMode::Vertical,
    ] {
        let renaming = tab_surface_tip_boxes(layout, &strip, Some(&rail), scale, false, Some(0));
        assert!(
            !ids(&renaming)
                .iter()
                .any(|id| matches!(id, tooltip::TooltipAnchorId::Tab(0))),
            "{layout:?}: a tab being renamed is not tipped"
        );
    }

    // A rail that is not on screen tips nothing, rather than falling through
    // to the strip that is also not on screen.
    assert!(
        tab_surface_tip_boxes(
            seats::TabLayoutMode::Vertical,
            &strip,
            None,
            scale,
            false,
            None
        )
        .is_empty()
    );
}

/// PIN — the profile picker hangs off the button that is **drawn**, and in
/// focus mode that button is the column's.
///
/// Red gate. `profile_menu_layout` computed the column's anchor first and
/// then walked into `self.rail_geometry_now(now)?` on its way to a fallback
/// it was about to discard — and `seats::rail_geometry` answers `None` for
/// every focus-mode window by design, so the `?` returned out of the whole
/// function. `toggle_profile_menu` had already flipped the state to open (the
/// chevron turned `^`), `refresh_overlay` gates the drawing on this layout
/// and drew nothing, and the press router gates on it too and so could
/// neither hit a row nor click the menu away. A dead switch, in exactly one
/// configuration: `focus_mode && layout == Vertical`.
///
/// The first assertion is the one that was red. The rest are the precedence
/// rule's other corners, kept beside it because a fix that answered the
/// column *instead of* the two ordinary surfaces would pass the first alone.
#[test]
fn the_profile_picker_hangs_off_the_new_tab_button_that_is_on_screen() {
    let scale = 1.0;
    let trailers = [seats::TabTrailer {
        pinned: false,
        reveal: 1.0,
        ..seats::TabTrailer::default()
    }];
    let strip = seats::tab_strip_geometry(960.0, scale, crate::seats::FOLIO_BAR, &trailers, 0, 0.0);
    // The three postures, named once and then used both to *measure* the
    // surfaces and to *ask* about them — a state that disagreed with the
    // geometry beside it would be a fixture no window can be in.
    let focus_state = seats::RailState {
        focus: true,
        ..rail_state_for(seats::TabLayoutMode::Vertical, seats::RailMode::Expanded)
    };
    let rail_state = seats::RailState {
        open: 1.0,
        ..rail_state_for(seats::TabLayoutMode::Vertical, seats::RailMode::Expanded)
    };
    let strip_state = rail_state_for(seats::TabLayoutMode::Horizontal, seats::RailMode::Expanded);
    let column = seats::focus_rail_geometry(
        600.0,
        scale,
        seats::FOLIO_BAR,
        trailers.len(),
        0,
        0.0,
        focus_state,
    )
    .expect("a focus-mode window draws its card column");
    let rail = seats::rail_geometry(
        600.0,
        scale,
        seats::FOLIO_BAR,
        &trailers,
        0,
        0.0,
        rail_state,
    )
    .expect("an expanded rail holding one tab is on screen");

    // The bug, stated: focus mode over a vertical layout has a card column
    // and **no** ordinary rail, and the menu still has a button to hang off.
    assert_eq!(
        profile_menu_anchor(Some(&column), None, strip.new_tab_menu, focus_state),
        Some((column.new_tab_menu, profiles::MenuSide::Beside)),
        "the column answers first, and a rail that is not drawn is not a \
             reason to answer nothing"
    );
    // The column supersedes the layout rather than combining with it
    // (`RailState::focus`'s own sentence): focus mode over a horizontal
    // strip hangs the menu off the column too, and beside it, not below.
    assert_eq!(
        profile_menu_anchor(
            Some(&column),
            None,
            strip.new_tab_menu,
            seats::RailState {
                focus: true,
                ..strip_state
            },
        ),
        Some((column.new_tab_menu, profiles::MenuSide::Beside)),
    );

    // And with no column, the two ordinary surfaces answer as they always
    // did — including the vertical layout's honest `None`.
    assert_eq!(
        profile_menu_anchor(None, Some(&rail), strip.new_tab_menu, rail_state),
        Some((
            rail.new_tab_menu.expect("an open rail draws its chevron"),
            profiles::MenuSide::Beside
        )),
    );
    assert_eq!(
        profile_menu_anchor(None, None, strip.new_tab_menu, rail_state),
        None,
        "a vertical window with no rail on screen has no button to hang it off"
    );
    // The same `None`, reached from the posture rather than from a missing
    // geometry: a folded rail has no width, so there is no button in it.
    assert_eq!(
        seats::RailState {
            collapsed: true,
            ..rail_state
        }
        .profile_menu_side(),
        None,
    );
    assert_eq!(
        profile_menu_anchor(None, Some(&rail), strip.new_tab_menu, strip_state),
        Some((strip.new_tab_menu, profiles::MenuSide::Below)),
        "and a horizontal one reads the strip, whatever a rail geometry says"
    );
}

/// PIN (user report 2026-08-18; `docs/DESIGN.md` §7.1.6c-4f) — **the rail's
/// panel survives the lift onto the overlay stack as a ground.**
///
/// The one-translucency ruling named the rail among the window's grounds and
/// [`seats::rail_chrome`] duly marks its panel [`ChromeSurface::Ground`] —
/// and then the class was thrown away one function later, because the rail
/// is not drawn in the chrome pass at all. Every quad came through
/// [`rail_overlay_layer`] as an `OverlayQuad { alpha: 1.0 }`, which is the
/// overlay's word for "opaque", so the fix landed on every band in the
/// window except this one: at 30% over a bright desktop the tab strip let it
/// through and the rail was a solid dark column.
///
/// Both halves are asserted, and both are load-bearing: routing *everything*
/// through the ground channel would take the hairline and the seam with it,
/// and a hairline drawn at the window's alpha is a hairline that dissolves
/// into the panel it is meant to separate.
///
/// Mutation: map every quad into `quads` as the lift used to, and the first
/// assertion finds no ground; map every quad into `grounds` and the second
/// finds no ink.
#[test]
fn the_rails_panel_reaches_the_overlay_as_a_ground_and_its_hairline_as_ink() {
    let panel = [0.0, 40.0, 220.0, 900.0];
    let hairline = [219.0, 40.0, 220.0, 900.0];
    let rail = seats::ChromeGroup {
        quads: vec![
            bt_render::ChromeQuad::ground(panel, [24, 24, 24]),
            bt_render::ChromeQuad::ink(hairline, [60, 60, 60]),
        ],
        labels: Vec::new(),
        sprites: Vec::new(),
        images: Vec::new(),
    };
    let layers = rail_overlay_layer(&rail, 1.0);
    let [layer] = layers.layers.as_slice() else {
        panic!("a rail with quads in it is one layer");
    };
    assert_eq!(
        layer.grounds,
        vec![bt_render::OverlayGround {
            rect: panel,
            color: [24, 24, 24],
        }],
        "`.rail {{ background: var(--panel) }}` is the window at that rectangle"
    );
    assert_eq!(
        layer.quads.iter().map(|quad| quad.rect).collect::<Vec<_>>(),
        vec![hairline],
        "the border-right is struck on the panel and stays opaque"
    );
    // The fold is the surface's, not the panel's: a ground has no alpha of
    // its own to fade, and lowering the surface's opacity must not be
    // answered by moving the panel out of the ground channel. Since ticket 46
    // the fold is the band's group, over the one layer drawn at full strength.
    let folding = rail_overlay_layer(&rail, 0.4);
    assert_eq!(folding.layers[0].grounds, layer.grounds);
    assert!((folding.layers[0].opacity - 1.0).abs() < 1e-6);
    assert_eq!(folding.groups.len(), 1);
    assert_eq!(folding.groups[0].layers, 0..1);
    assert!((folding.groups[0].opacity - 0.4).abs() < 1e-6);
    // An empty rail is no layer at all, which is what a horizontal layout
    // and a collapsed rail both hand this function.
    assert!(rail_overlay_layer(&seats::ChromeGroup::default(), 1.0).is_empty());
}

/// PIN (§7.1.6b′, user report on the real machine 2026-08-19) — **the card
/// column is never faded by a fold that is not its own.**
///
/// `Sidebar`'s three rest states govern the *ordinary* rail; the column is
/// card-width and fully drawn whatever they say, and [`panel_opacity`] is
/// where those two facts meet. The failure this closes was photographed:
/// fold the sidebar away, then enter focus mode, and the window solved a
/// card-wide column, started the stage after it, hit-tested its cards — and
/// painted the layer at `opacity: 0`. A live, invisible panel.
///
/// Red gate: return the fold unconditionally and the first loop goes red at
/// every folded value; drop the `else` and a *collapsing ordinary* rail pops
/// instead of fading, which the second half asserts.
#[test]
fn the_card_column_is_never_faded_by_a_fold_that_is_not_its_own() {
    for layout in [
        seats::TabLayoutMode::Horizontal,
        seats::TabLayoutMode::Vertical,
    ] {
        for mode in [seats::RailMode::Expanded, seats::RailMode::Icons] {
            for collapsed in [false, true] {
                let ordinary = seats::RailState {
                    layout,
                    mode,
                    collapsed,
                    ..seats::RailState::default()
                };
                let focused = seats::RailState {
                    focus: true,
                    ..ordinary
                };
                for fold in [0.0, 0.4, 1.0] {
                    assert_eq!(
                        panel_opacity(focused, fold),
                        1.0,
                        "{layout:?}/{mode:?}/collapsed={collapsed}: the column is \
                             drawn whole however far the sidebar's own fold has \
                             travelled"
                    );
                    assert_eq!(
                        panel_opacity(ordinary, fold),
                        fold,
                        "{layout:?}/{mode:?}/collapsed={collapsed}: while the \
                             ordinary rail still travels on its own clock"
                    );
                }
            }
        }
    }
}

/// **Red gate (§7.1.6e″ ③): a sidebar that goes away takes its menus with
/// it.**
///
/// The other half of the fix. Rule ① means the hover zone can no longer
/// retract out from under a menu the rail raised — but the fold and the two
/// posture rows do not go past the zone at all, so without this a
/// `Ctrl+B` under an open profile list leaves a menu that draws nothing,
/// swallows the keyboard and cannot be clicked away.
///
/// Mutation: drop the `collapsed` clause and the fold row goes red; drop the
/// `(layout, mode)` clause and both posture rows do.
#[test]
fn a_rail_that_folds_away_or_moves_house_strands_no_menu() {
    let icons = seats::RailState {
        layout: seats::TabLayoutMode::Vertical,
        mode: seats::RailMode::Icons,
        ..seats::RailState::default()
    };

    assert!(
        !rail_change_strands_its_popups(icons, icons),
        "a posture that did not move takes nothing down"
    );
    assert!(
        rail_change_strands_its_popups(
            icons,
            seats::RailState {
                collapsed: true,
                ..icons
            }
        ),
        "the fold takes the whole panel off the screen, so the menu hanging \
             off its `˅` has no button left"
    );
    assert!(
        rail_change_strands_its_popups(
            icons,
            seats::RailState {
                mode: seats::RailMode::Expanded,
                ..icons
            }
        ),
        "and a rail rebuilt in the other mode is born parked at the top of \
             its list — the button has moved"
    );
    assert!(
        rail_change_strands_its_popups(
            icons,
            seats::RailState {
                layout: seats::TabLayoutMode::Horizontal,
                ..icons
            }
        ),
        "the tab list moving to the strip hands the `+` to another surface"
    );

    let folded = seats::RailState {
        collapsed: true,
        ..icons
    };
    assert!(
        !rail_change_strands_its_popups(folded, icons),
        "un-folding strands nothing: the panel is arriving, not leaving"
    );
}

/// Every rail state the settings dialog can ask for arrives with both of the
/// rail's scalars at their defaults.
///
/// This is the half that would rot silently: a rail that inherited the last
/// icon rail's `open: 1.0` would appear already wide with the pointer nowhere
/// near it, and would then never close, because the zone trigger only fires
/// on a pointer that *moves*.
#[test]
fn every_rail_state_is_born_with_its_scalars_at_rest() {
    use seats::{RailMode, TabLayoutMode};
    let start = seats::RailState::default();
    assert_eq!(start.layout, TabLayoutMode::Horizontal);
    assert_eq!(
        start,
        rail_state_for(TabLayoutMode::Horizontal, RailMode::Expanded),
        "the default rail is the one the dialog builds for the same pair"
    );

    let expanded = rail_state_for(TabLayoutMode::Vertical, RailMode::Expanded);
    assert_eq!(
        expanded.terminal_inset_logical_px(),
        bt_render::RAIL_WIDTH_LOGICAL_PX,
        "an expanded rail is in the flow and takes its width out of the terminal's"
    );

    let icons = rail_state_for(TabLayoutMode::Vertical, RailMode::Icons);
    assert_eq!(icons.open, 0.0, "an icon rail is born parked");
    assert_eq!(icons.text_opacity, 0.0, "with its words away");
    assert_eq!(
        icons.terminal_inset_logical_px(),
        bt_render::RAIL_PARK_LOGICAL_PX,
        "Q179: the terminal keeps only the parked strip clear"
    );

    // A rail caught mid-animation still resolves to the same state: the
    // scalars are what the builder resets, not what it reads.
    let mid = seats::RailState {
        open: 0.6,
        text_opacity: 0.3,
        ..icons
    };
    assert_eq!(rail_state_for(mid.layout, mid.mode), icons);
}

/// PIN (Bug 4): the panel-toggle's fold travels on P168's own curve.
///
/// The defect this pins was a pair of omissions, and the test names both.
/// `set_rail_state` started a tween only when `(layout, mode)` moved, so
/// flipping `collapsed` started none; and `width_logical_px` short-circuited
/// to `0.0` on the flag, so even a running tween would have had nothing to
/// scale. The rail therefore left in one frame while the *same* declaration
/// — `.rail { transition: width .18s ease }` — was easing the hover open.
#[test]
fn folding_the_rail_away_travels_over_the_rails_own_transition() {
    let expanded = seats::RailState {
        layout: seats::TabLayoutMode::Vertical,
        mode: seats::RailMode::Expanded,
        ..seats::RailState::default()
    };
    // At rest the fold is `collapsed` read as a number, which is the constant
    // the old short-circuit returned — so nothing that never mentions a fold
    // can have changed width.
    assert_eq!(
        expanded.width_logical_px(),
        bt_render::RAIL_WIDTH_LOGICAL_PX
    );
    assert_eq!(
        seats::RailState {
            collapsed: true,
            ..expanded
        }
        .width_logical_px(),
        0.0,
        "a collapsed rail at rest is still exactly gone"
    );
    // Halfway through, it is halfway out — the state the snap never had.
    assert_eq!(
        seats::RailState {
            collapsed: true,
            fold: Some(0.5),
            ..expanded
        }
        .width_logical_px(),
        bt_render::RAIL_WIDTH_LOGICAL_PX / 2.0,
        "the fold scales the width instead of switching it off"
    );
    // The horizontal layout still owns the first word, fold or no fold: a
    // rail that is not on this axis cannot be eased onto it.
    assert_eq!(
        seats::RailState {
            layout: seats::TabLayoutMode::Horizontal,
            fold: Some(1.0),
            ..expanded
        }
        .width_logical_px(),
        0.0
    );

    // And the clock it travels on is `.rail`'s own 180ms, moving at 30ms and
    // arrived at 180 — the same span and curve the hover open already uses.
    let start = Instant::now();
    let mut fold = RevealTween::resting(1.0, RAIL_TRANSITION);
    fold.retarget(0.0, start, Motion::Full);
    let (partway, moving) = fold.sample(start + Duration::from_millis(30), Motion::Full);
    assert!(
        partway < 1.0 && partway > 0.0,
        "30ms into the fold the panel is on its way, not gone: {partway}"
    );
    assert!(moving, "and it owes the next frame");
    assert_eq!(
        fold.sample(start + RAIL_TRANSITION, Motion::Full),
        (0.0, false),
        "at 180ms it has arrived and asks for nothing more"
    );
}

/// The pure half of `solve_seats` (main.rs's own free function), parameterized on a
/// `dpi_milli` instead of `&Renderer` so a startup scenario can be solved without a live GPU
/// device. Numerically identical to what `Runtime::create` and `resolve_seat_layout` do with
/// an actual renderer in hand — see `solve_seats`'s own body.
fn solved_terminal_seat(
    seats: &seats::Seats,
    dpi_milli: u32,
    render_physical: PhysicalSize<u32>,
) -> bt_render::SeatViewport {
    let metrics = seats::seat_metrics(dpi_milli);
    let viewport = seats::logical_viewport(
        render_physical.width,
        render_physical.height,
        seats::scale_ppm(dpi_milli),
        0,
        seats::folio_band_device_px(seats::scale_ppm(dpi_milli)),
    );
    let layout = match seats.solve(viewport, &metrics, SizePolicy::Lawful) {
        Ok(layout) => layout,
        Err(_) => seats::fit_what_fits(seats, viewport, &metrics).0,
    };
    seats::pane_body_viewport(seats, &layout, seats.identity(), dpi_milli as f32 / 1_000.0)
        .unwrap_or(bt_render::SeatViewport::whole(
            render_physical.width.max(1),
            render_physical.height.max(1),
        ))
}

/// Crossing the one/two-pane boundary changes terminal rows immediately,
/// while each resulting ConPTY size still waits on the shared 200ms quiet
/// window used by ordinary window and divider resizes.
#[test]
fn pane_count_boundary_reflows_rows_through_the_existing_resize_coalescer() {
    let dpi_milli = 1_000;
    let physical = PhysicalSize::new(1600, 900);
    let metrics = seats::seat_metrics(dpi_milli);
    let mut seats = seats::Seats::lone_terminal();
    let lone = solved_terminal_seat(&seats, dpi_milli, physical);

    let preview = seats
        .add_preview(&metrics)
        .expect("a 1600x900 tab seats the ruled preview");
    let split = solved_terminal_seat(&seats, dpi_milli, physical);
    assert_eq!(lone.y, 40);
    assert_eq!(lone.height, 860, "lone terminal body is the whole seat");
    // The head's own height, read from the constant rather than restated —
    // it moved from 28 to 30 on 2026-08-12 and this assertion is about the
    // body giving up exactly a head, not about the number.
    let head = bt_render::SEAT_TITLE_BAR_LOGICAL_PX as u32;
    assert_eq!(split.y, lone.y + head);
    assert_eq!(split.height, lone.height - head);

    // Representative renderer metrics make the viewport-to-grid boundary
    // explicit here; CellMetrics::grid_for_pixels owns the same floor.
    let rows_for = |height: u32| ((height.saturating_sub(16)) / 20).max(1) as u16;
    let lone_grid = grid_of(100, rows_for(lone.height));
    let split_grid = grid_of(100, rows_for(split.height));
    assert!(split_grid.rows < lone_grid.rows);

    let start = Instant::now();
    let mut pending = None;
    assert!(coalesce_pty_resize_on_grid_change(
        &mut pending,
        split_grid,
        lone_grid,
        lone_grid,
        PhysicalSize::new(split.width, split.height),
        start,
    ));
    assert!(
        take_due_pty_resize(
            &mut pending,
            start + WINDOW_RESIZE_QUIET - Duration::from_millis(1)
        )
        .is_none()
    );
    assert_eq!(
        take_due_pty_resize(&mut pending, start + WINDOW_RESIZE_QUIET)
            .unwrap()
            .grid,
        split_grid
    );

    assert!(seats.close_seat(&metrics, preview));
    let closed = solved_terminal_seat(&seats, dpi_milli, physical);
    assert_eq!(closed, lone);
    let close_at = start + Duration::from_secs(1);
    assert!(coalesce_pty_resize_on_grid_change(
        &mut pending,
        lone_grid,
        split_grid,
        split_grid,
        PhysicalSize::new(closed.width, closed.height),
        close_at,
    ));
    assert_eq!(
        take_due_pty_resize(&mut pending, close_at + WINDOW_RESIZE_QUIET)
            .unwrap()
            .grid,
        lone_grid
    );
}

/// PIN (startup order): a session restore with a preview seat open must spawn ConPTY at the
/// seat's own grid and ask it for nothing more.
///
/// `Runtime::create` resolves the seat layout (`seats::Seats::from_persisted` -> `solve_seats`)
/// *before* `PtySession::spawn_default`, and seeds `self.grid` to that same solve's grid. §4.2
/// says solve is pure, so the very first re-solve after `ShowWindow`
/// (`reconcile_authoritative_dpi`) — run against the identical tree and an identical,
/// same-DPI viewport — reproduces the identical seat rectangle, and therefore the identical
/// `GridSize`. `coalesce_pty_resize_on_grid_change` is the single point every later solve
/// (a live OS `Resized`, a divider drag, a DPI reconciliation) funnels through; fed the exact
/// pair a clean restore produces, it must schedule nothing.
#[test]
fn a_restored_split_tree_reaches_zero_pty_resize_requests_after_a_matching_dpi_spawn() {
    // The shape a preview-narrowed terminal round-trips to `session.json` as: a row split,
    // the terminal first, a pinned preview second (`seats.rs`'s `LeafNodeV1::Preview` docs).
    let node = bt_persist::LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
        dir: bt_persist::SplitDirV1::Row,
        ratio: 700_000,
        children: [
            Box::new(bt_persist::LayoutNodeV1::Leaf(
                bt_persist::LeafNodeV1::Term(bt_persist::TermLeafV1 {
                    profile_id: "pwsh.exe".to_owned(),
                    cwd: String::new(),
                    manual_name: None,
                    card_skip: 0,
                    last_command: String::new(),
                }),
            )),
            Box::new(bt_persist::LayoutNodeV1::Leaf(
                bt_persist::LeafNodeV1::Preview(bt_persist::PreviewLeafV1 { pinned: true }),
            )),
        ],
    });
    let seats = seats::Seats::from_persisted(&node);
    let dpi_milli = 1_000_u32; // the restored session's recorded DPI equals the monitor's at show
    let render_physical = PhysicalSize::new(1600, 900);

    // The spawn-time solve (before `PtySession::spawn_default`) and the post-`ShowWindow`
    // re-solve, run back to back exactly as startup does.
    let spawn_rect = solved_terminal_seat(&seats, dpi_milli, render_physical);
    let resolved_rect = solved_terminal_seat(&seats, dpi_milli, render_physical);
    assert_eq!(
        spawn_rect, resolved_rect,
        "an unchanged tree against an unchanged viewport must solve to the same seat twice"
    );
    assert!(
        spawn_rect.width < render_physical.width,
        "the preview seat must actually narrow the terminal, or this pin proves nothing"
    );

    // `CellMetrics::grid_for_pixels` is a pure function of the seat rectangle and the
    // (unchanged) font metrics, so an identical rectangle answers an identical `GridSize` on
    // both solves — that arithmetic is already pinned in `bt-render`. What this test pins is
    // the gate downstream of it, fed the one pair of grids a clean restore ever produces.
    let seat_grid = GridSize {
        columns: std::num::NonZeroU16::new(100).unwrap(),
        rows: std::num::NonZeroU16::new(30).unwrap(),
    };
    let physical = PhysicalSize::new(spawn_rect.width, spawn_rect.height);

    // `Runtime::create` seeds `self.grid` to exactly the spawn-time grid; the first
    // post-show solve compares against that same value.
    let current_grid = seat_grid;
    let mut pending = None;
    let now = Instant::now();
    let scheduled = coalesce_pty_resize_on_grid_change(
        &mut pending,
        seat_grid,
        current_grid,
        // `Runtime::create` seeds the actor's grid from the same solve, so the restore moves
        // neither grid: nothing is told, and nothing is queued to be released either.
        current_grid,
        physical,
        now,
    );
    assert!(
        !scheduled,
        "the first post-spawn solve answers the exact grid the PTY was spawned with"
    );
    assert!(
        take_due_pty_resize(&mut pending, now + WINDOW_RESIZE_QUIET).is_none(),
        "spawn size already equals the seat grid; a matching-DPI restore must schedule zero \
             ConPTY resizes"
    );
}

/// PIN (P0, real-machine): the pane holding the keyboard is sized from **its
/// own** body, never from the tab's primary seat.
///
/// The reported bug. `resolve_seat_layout` used to answer the grid of
/// `seats.identity()` — a stored field naming one fixed leaf, which moves
/// only when that leaf closes and never with focus — and all four callers
/// handed that answer to `schedule_grid_change`, which writes through the
/// `Runtime -> TabState -> LeafSession` deref chain to the *focused* leaf. A
/// lone-terminal tab hid it because the two seats are then the same
/// rectangle. Split the tab, focus the narrow pane, and its shell was told
/// the wide pane's column count: long lines ran off the right edge instead
/// of wrapping, and PSReadLine's anchor arithmetic — which trusts the
/// reported buffer width — raised "the value must be greater than or equal
/// to zero and less than the console's buffer size".
///
/// MUTATION: hand the focused target `pane_body_viewport(.., seats.identity())`
/// — the exact pre-fix source — and the width assertions below go red.
#[test]
fn the_pane_holding_the_keyboard_is_sized_from_its_own_body_not_the_primary_seat() {
    let dpi_milli = 1_000;
    let render_physical = PhysicalSize::new(1600, 900);
    let scale = dpi_milli as f32 / 1_000.0;
    let (seats, layout) = solved_lopsided_split(dpi_milli, render_physical);

    let leaves = seats.terminals();
    assert_eq!(leaves.len(), 2, "the fixture is a two-pane tab");
    let primary = seats.identity();
    let narrow = leaves
        .iter()
        .copied()
        .find(|seat| *seat != primary)
        .expect("the second leaf is not the primary seat");
    let primary_body = seats::pane_body_viewport(&seats, &layout, primary, scale)
        .expect("the solver placed the primary pane");
    let narrow_body = seats::pane_body_viewport(&seats, &layout, narrow, scale)
        .expect("the solver placed the second pane");
    assert!(
        narrow_body.width * 2 < primary_body.width,
        "the two panes must differ sharply or this pin proves nothing: \
             {} against {}",
        narrow_body.width,
        primary_body.width
    );

    // The keyboard is in the narrow pane — the reported case.
    let plan = leaf_resize_plan(&seats, &layout, narrow, scale);
    let focused = plan
        .iter()
        .copied()
        .find(|target| target.focused)
        .expect("the focused leaf is in the plan");
    assert_eq!(focused.seat, narrow);
    assert_eq!(
        focused.body, narrow_body,
        "the focused pane is sized from its own body"
    );
    assert!(
        focused.body.width < primary_body.width,
        "the focused narrow pane must not be handed the primary pane's width: \
             {} against {}",
        focused.body.width,
        primary_body.width
    );

    // Every leaf, focused or not, gets its own rectangle and no other's.
    for target in &plan {
        assert_eq!(
            Some(target.body),
            seats::pane_body_viewport(&seats, &layout, target.seat, scale),
            "leaf {:?} is sized from its own pane body",
            target.seat
        );
    }
}

/// PIN (user report, 2026-08-13): **a seat arriving narrows the shells
/// beside it, and the arrival is legible from the tree alone.**
///
/// The audit behind `Runtime::settle_seat_set_change` needed two facts to be
/// true of every seat-set change, and this pins both for the preview seat —
/// the one the report was made against:
///
/// 1. the terminal's *solved body genuinely shrinks*, so a shell that is not
///    re-sized from the new solve is a shell living at a width the window no
///    longer draws. This is the whole of the reported symptom: a prompt
///    reprinting itself into columns that are not there loses its middle;
/// 2. `structure_revision` moves, which is what `shells_settled_revision`
///    compares itself against. The revision is the only thing that can tell
///    a frame "the seat set changed and nobody carried it", because every
///    other reading is a reading of the *caller's* intent — and intent is
///    exactly what the two verbs that skipped the ceremony had plenty of.
///
/// MUTATION ①: drop the `structure_revision += 1` from `Seats::add_preview`
/// and the revision assertion goes red — which is the tripwire going blind,
/// the same blindness that let `dock_float` re-solve and stop.
/// MUTATION ②: solve the *pre-landing* tree for `after` and the width
/// assertion goes red, which is the pre-fix `dock_float` in one line: the
/// tree grew a pane and the rectangle handed to the shell did not.
#[test]
fn a_preview_seat_arriving_narrows_the_terminal_and_moves_the_revision() {
    let dpi_milli = 1_000_u32;
    let scale = dpi_milli as f32 / 1_000.0;
    let metrics = seats::seat_metrics(dpi_milli);
    let viewport = seats::logical_viewport(
        1600,
        900,
        seats::scale_ppm(dpi_milli),
        0,
        seats::folio_band_device_px(seats::scale_ppm(dpi_milli)),
    );
    let solve = |seats: &seats::Seats| {
        seats
            .solve(viewport, &metrics, SizePolicy::Lawful)
            .expect("a 1600x900 window seats a terminal beside a preview")
    };

    let mut seats = seats::Seats::lone_terminal();
    let terminal = seats.identity();
    let before_layout = solve(&seats);
    let before = seats::pane_body_viewport(&seats, &before_layout, terminal, scale)
        .expect("the solver placed the lone terminal");
    let settled = seats.structure_revision();

    seats
        .add_preview(&metrics)
        .expect("a 1600x900 lone-terminal tab has room for the ruled preview seat");
    let after_layout = solve(&seats);
    let after = seats::pane_body_viewport(&seats, &after_layout, terminal, scale)
        .expect("the solver placed the terminal beside the preview");

    assert!(
        after.width < before.width,
        "the terminal must actually give up width to the arriving preview, or \
             there is no bug here to have: {} against {}",
        after.width,
        before.width
    );
    assert_ne!(
        seats.structure_revision(),
        settled,
        "the arrival has to move the revision the frame-time tripwire reads"
    );

    // And the carry itself answers the *new* rectangle for the leaf it is
    // about — the step `dock_float` was missing entirely.
    let plan = leaf_resize_plan(&seats, &after_layout, terminal, scale);
    assert_eq!(plan.len(), 1, "one terminal leaf, one carry");
    assert_eq!(
        plan[0].body, after,
        "the shell is told the rectangle the preview left it, not the one it had"
    );
}

/// **K115 in the rail: a grabbed row is held inside the list's own viewport
/// and cannot be carried out past its top or its foot.**
///
/// `grabbed_offset` never knew which axis it was on — it clamps a leading
/// edge against a `[start, end]` pair — so what is new here is what the rail
/// hands it: the *list's* clip box rather than the window's, and the row's
/// height rather than a tab's width. The two bounds are asserted to be
/// strictly inside the window, because that is the difference between a row
/// stopped by the scroller and a row stopped by the framebuffer — the
/// heading is above the first and the `+` row is below the second, and a
/// grabbed row that could reach either would be drawn over furniture that is
/// not part of the list.
///
/// Red gate: project the run on `x` and `start`/`extent` come back as the
/// row's left edge and its 203px width, so every clamp below lands on a
/// different number.
#[test]
fn a_grabbed_row_is_held_inside_the_rails_own_viewport() {
    const HEIGHT: f32 = 618.0;
    // Enough rows that the list actually overflows: a rail with room to
    // spare has a viewport its rows never reach, and a clamp nothing tests.
    let trailers = vec![seats::TabTrailer::default(); 40];
    let rail = seats::rail_geometry(
        HEIGHT,
        1.0,
        seats::FOLIO_BAR,
        &trailers,
        0,
        0.0,
        seats::RailState {
            layout: seats::TabLayoutMode::Vertical,
            mode: seats::RailMode::Expanded,
            collapsed: false,
            open: 1.0,
            text_opacity: 1.0,
            fold: None,
            focus: false,
            focus_card_body_logical_px: bt_render::DEFAULT_FOCUS_MINI_HEIGHT_LOGICAL_PX,
        },
    )
    .expect("an expanded rail is on screen");
    assert!(
        rail.max_scroll > 0.0,
        "the list overflows, so it has a foot"
    );
    let run = seats::rail_run(&rail);
    let [top, foot] = run.viewport;
    assert!(
        top > 0.0 && foot < HEIGHT,
        "the clip is the list's own box, not the window's: {:?}",
        run.viewport
    );
    // A row partway down the list rather than the first: the head's own top
    // edge already sits on the viewport's, so clamping it up is a move that
    // could be got right by doing nothing.
    const ROW: usize = 5;
    let start = run.start(ROW).expect("a row partway down the list");
    let height = run.extent(ROW).expect("and its height");
    // Everything is asserted through the row's *drawn* box, which is what
    // `RailTabGeometry::shifted` will move: the offset is a number nobody
    // looks at, and the promise is about where the row you can see ends up.
    let drawn = |offset: f32| {
        let row = rail.tabs[ROW].shifted(offset);
        [row.body[1], row.body[3]]
    };
    assert_eq!(
        grabbed_offset(start, height, run.viewport, start + 90.0),
        90.0,
        "free of both ends, the offset is simply the distance travelled"
    );
    assert_eq!(
        drawn(grabbed_offset(start, height, run.viewport, -5_000.0)),
        [top, top + height],
        "carried up past the head it stops with its top edge on the list's"
    );
    assert_eq!(
        drawn(grabbed_offset(start, height, run.viewport, 5_000.0)),
        [foot - height, foot],
        "and carried down past the foot with its bottom edge on that one"
    );
}

/// The focus column of a window holding `tabs` tabs, solved — the geometry
/// [`Runtime::tab_run`] hands the drag engine while the mode is on.
fn focus_column(tabs: usize) -> seats::FocusRailGeometry {
    seats::focus_rail_geometry(
        618.0,
        1.0,
        seats::FOLIO_BAR,
        tabs,
        0,
        0.0,
        seats::RailState {
            layout: seats::TabLayoutMode::Vertical,
            mode: seats::RailMode::Expanded,
            collapsed: false,
            open: 1.0,
            text_opacity: 1.0,
            fold: None,
            focus: true,
            focus_card_body_logical_px: bt_render::DEFAULT_FOCUS_MINI_HEIGHT_LOGICAL_PX,
        },
    )
    .expect("focus mode puts a column on screen")
}

/// **§7.1.6b′ ④ is spent: a card is reordered by the tab strip's own engine,
/// and it moves the one list there is** (2026-08-20).
///
/// The ruling used to read *"v1 不做卡片拖动排序"*, and the reason it gave was
/// a condition rather than a refusal: reordering by card had to arrive
/// carrying the tab strip's semantics instead of growing a second set. R3
/// delivered that condition — [`Runtime::tab_run`] is the one place the
/// surface is chosen and everything under it takes a [`seats::TabRun`] — so
/// what is asserted here is that the column *is* one of those surfaces and
/// nothing about the judgement changed when it became one.
///
/// Every number below comes out of the run: the mids the swap is judged
/// against, the half-card of travel it costs, and the pitch. The card's
/// height is deliberately not named — F2 grew it once and the projection
/// slice is growing it again, and a test that named it would be pinning the
/// constant instead of the mechanism.
///
/// The move itself is the two lines [`Runtime::move_tab_with_flip`] performs
/// on `window.tabs`, which is the whole of "the strip and the column cannot
/// disagree": there is no `cardSeq` to reconcile, so a reorder driven from a
/// card is a reorder of the list the strip reads and the session writes.
///
/// Red gate: hand the column an empty `slots` list — which is what
/// `focus_rail_run` did until 2026-08-20 — and `mids` comes back empty, so
/// `half` is `None` and the drag computes nothing at all.
#[test]
fn a_card_dragged_past_its_neighbours_midline_reorders_the_one_tab_list() {
    const COUNT: usize = 4;
    let column = focus_column(COUNT);
    let run = seats::focus_rail_run(&column);
    assert_eq!(
        run.slots.len(),
        COUNT,
        "one slot per card — the offer of a place to land"
    );
    let mids = run.mids();
    let half = run
        .half(0)
        .expect("a card has a half-height to be judged by");
    let unpinned = [false; COUNT];

    // The third card carried to the head of the column: the same fling the
    // strip answers by walking one slot at a time.
    assert_eq!(
        seats::reorder_target(&mids, &unpinned, 2, mids[0], half),
        0,
        "a card flung to the top of the column lands at the top"
    );
    // And one neighbour's worth of travel, which is where the threshold is:
    // the leading edge has to cover half of the card below plus the margin.
    let pitch = mids[1] - mids[0];
    assert_eq!(
        seats::reorder_target(&mids, &unpinned, 0, mids[0] + pitch, half),
        1,
        "a card dragged one slot down swaps with the card it covered"
    );
    assert_eq!(
        seats::reorder_target(&mids, &unpinned, 0, mids[0], half),
        0,
        "and a card that has not moved does not"
    );

    // The list the move is performed on, exactly as `move_tab_with_flip`
    // performs it.
    let mut tabs = ["alpha", "beta", "gamma", "delta"];
    let to = seats::reorder_target(&mids, &unpinned, 2, mids[0], half);
    let moved = tabs[2];
    tabs.copy_within(to..2, to + 1);
    tabs[to] = moved;
    assert_eq!(
        tabs,
        ["gamma", "alpha", "beta", "delta"],
        "the reorder rewrites `window.tabs`, which is the list the strip \
             draws and the session stores — there is no second sequence"
    );
}

/// **F57 in the card column: pinned is a partition there too.**
///
/// The guard lives inside [`seats::reorder_target`] and is index-based, so a
/// column inherits it by being a run — but the column is the one surface
/// that draws no `.pin-seam` (Q187 keeps that line in the rail alone), so
/// the rule holding here is the only thing that stops a card being dragged
/// across a boundary nothing on screen is drawing.
///
/// Red gate: pass `&[false; 4]` for the pins and the first assertion walks
/// straight across the seam.
#[test]
fn a_card_reorder_stops_dead_at_the_pinned_seam() {
    const COUNT: usize = 4;
    let column = focus_column(COUNT);
    let run = seats::focus_rail_run(&column);
    let mids = run.mids();
    let half = run
        .half(0)
        .expect("a card has a half-height to be judged by");
    // Two pinned cards at the head, two loose ones under them — the
    // screenshot's own arrangement.
    let pinned = [true, true, false, false];
    assert_eq!(
        seats::reorder_target(&mids, &pinned, 3, mids[0], half),
        2,
        "a loose card flung at the top of the column stops under the pins"
    );
    assert_eq!(
        seats::reorder_target(&mids, &pinned, 0, mids[3], half),
        1,
        "and a pinned card flung at the foot stops at the seam from above"
    );
    assert_eq!(
        seats::reorder_target(&mids, &pinned, 0, mids[1], half),
        1,
        "while inside a partition it moves freely"
    );
}

/// **K115 in the card column: a grabbed card is held inside the list's own
/// clip box**, the same clamp the rail's rows answer to.
///
/// `grabbed_offset` has never known which surface it is on — it clamps a
/// leading edge against a `[start, end]` pair — so what is new is only what
/// the column hands it. The clip is the *list's* box and not the panel's: the
/// panel's top margin is above it and the sticky `+` row is below it, and a
/// card that could reach either would be drawn over furniture that does not
/// scroll.
///
/// Red gate: hand the run the panel's box instead of the list's clip and the
/// card carried down covers the `+` row.
#[test]
fn a_grabbed_card_is_held_inside_the_columns_own_viewport() {
    // Enough cards that the list actually overflows: a column with room to
    // spare has a viewport its cards never reach, and a clamp nothing tests.
    let column = focus_column(40);
    assert!(
        column.max_scroll > 0.0,
        "the list overflows, so it has a foot"
    );
    let run = seats::focus_rail_run(&column);
    let [top, foot] = run.viewport;
    assert!(
        top == column.cards[0].body[1] && foot < 618.0,
        "the clip is the list's own box, opening on the first card: {:?}",
        run.viewport
    );
    // A card partway down rather than the first: the head card's own top
    // edge already sits on the viewport's, so clamping it up is a move that
    // could be got right by doing nothing.
    const CARD: usize = 3;
    let start = run.start(CARD).expect("a card partway down the column");
    let height = run.extent(CARD).expect("and its height");
    let drawn = |offset: f32| {
        let card = column.cards[CARD].shifted(offset);
        [card.body[1], card.body[3]]
    };
    assert_eq!(
        drawn(grabbed_offset(start, height, run.viewport, -5_000.0)),
        [top, top + height],
        "carried up past the head it stops with its top edge on the list's"
    );
    assert_eq!(
        drawn(grabbed_offset(start, height, run.viewport, 5_000.0)),
        [foot - height, foot],
        "and carried down past the `+` row with its bottom edge on that one"
    );
}

/// PIN — U8, R3. A pane growing into a closed sibling's space is *clipped*
/// into it, never stretched into it.
///
/// The complement of the split above and the half where the FLIP is actually
/// visible: the survivor's contents are laid out at their final, larger size
/// from the corner they still occupy, and the animating box opens over them
/// like a curtain. On the first frame the box is strictly smaller than the
/// contents — that is the clipping — and by the last both are the rectangle
/// the solver gave.
///
/// The viewport legitimately hangs off the right of the surface while this
/// runs (`x + width` past the window), which is why only the scissor of the
/// pair is clamped and why it is clamped where the device validates it.
#[test]
fn a_pane_growing_out_of_a_closed_sibling_is_clipped_into_the_space_not_stretched_into_it() {
    let now = Instant::now();
    let metrics = seats::seat_metrics(1_000);
    let viewport = seats::logical_viewport(
        1600,
        900,
        seats::scale_ppm(1_000),
        0,
        seats::folio_band_device_px(seats::scale_ppm(1_000)),
    );
    let (mut seats, _, split, survivor, arriving) = split_window(true);
    let before = split;
    assert!(seats.close_seat(&metrics, arriving), "the left pane closes");
    let after = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("the survivor solves");

    let mut motion = PaneMotion::default();
    motion.begin(
        &pane_rects_of(&before),
        &pane_rects_of(&after),
        now,
        Motion::Full,
    );
    let pane = pane_box_of(&after, survivor);
    let body =
        seats::pane_body_viewport(&seats, &after, survivor, 1.0).expect("the survivor has a body");

    let first =
        animated_pane_viewports(body, pane, motion.transform_of(survivor, now, Motion::Full));
    let (content, clip) = first;
    assert!(
        clip.width < content.width,
        "the box the survivor is drawn through is still the narrow one it \
             had ({} against {} of content)",
        clip.width,
        content.width
    );
    assert_eq!(
        content.width, body.width,
        "while the contents are at their final width from the first frame"
    );
    assert!(
        content.x + content.width > 1600,
        "and that puts the viewport off the right of a 1600px surface, which \
             is legal for a viewport and is why the scissor is the clamped one"
    );

    let (landed_content, landed_clip) = animated_pane_viewports(
        body,
        pane,
        motion.transform_of(survivor, now + PANE_FLIP, Motion::Full),
    );
    assert_eq!(
        landed_content, body,
        "the flight ends on the solver's answer"
    );
    assert_eq!(
        landed_clip, body,
        "and the box it is drawn through has opened all the way to it"
    );
}

/// PIN — U8, R5. Only a structural tree change animates.
///
/// Three assertions because there are three cases and they are not variants
/// of one: a divider drag steers a ratio, a focus change feeds W2's
/// concession ladder, and a split adds a leaf. All three re-solve, and the
/// first two move rectangles — which is exactly why "the layout re-solved"
/// cannot be the gate, and why the middle assertion carries the search that
/// proves a focus change really does move them rather than asserting it.
///
/// A gate on `Edit`-ness rather than on displacement is also what makes
/// `CenterSwap` come out right: it is counted as structural and then moves
/// nothing, so P178 skips every pane and the swap is not animated — reached
/// by the rule instead of by an exception carved out of it.
#[test]
fn a_divider_drag_and_a_focus_change_start_no_pane_tween_and_a_split_starts_one() {
    let metrics = seats::seat_metrics(1_000);
    let viewport = seats::logical_viewport(
        1600,
        900,
        seats::scale_ppm(1_000),
        0,
        seats::folio_band_device_px(seats::scale_ppm(1_000)),
    );
    let (mut seats, _, split, survivor, arriving) = split_window(true);

    // A split: the shape changed, so the gate fires.
    assert_eq!(
        seats.structure_revision(),
        1,
        "the split is the one edit here that added a leaf"
    );

    // A divider drag: rectangles move, the shape does not.
    let at_rest = seats.structure_revision();
    let slot = *seats
        .split_slots(&split)
        .first()
        .expect("the split has a divider");
    let usable = slot.slot.extent(slot.dir) - bt_layout::DIVIDER;
    assert_eq!(
        seats.drag_divider(
            &metrics,
            slot.id,
            bt_layout::Ratio::clamped_from_ppm(300_000),
            usable
        ),
        Ok(true),
        "the drag has to actually write a ratio, or this assertion is about \
             a refusal instead of about the gate"
    );
    let dragged = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("the drag solves");
    assert_ne!(
        pane_box_of(&dragged, survivor),
        pane_box_of(&split, survivor),
        "the drag moved the pane's rectangle"
    );
    assert_eq!(
        seats.structure_revision(),
        at_rest,
        "and moved no leaf, so nothing animates: a 200ms tail on a gesture \
             the pointer is already driving frame by frame"
    );

    // A focus change: rectangles move through the concession ladder, the
    // shape does not. The width is searched for rather than asserted,
    // because "focus moves rectangles" is a fact about W2 and about this
    // build's minimum sizes, not a number this test may invent.
    let mut narrow = seats::Seats::lone_terminal();
    let first = narrow.identity();
    let second = narrow
        .split_terminal(&metrics, first, bt_layout::Axis::Row, false)
        .expect("a wide window divides");
    let ladder = (200..900).step_by(4).find_map(|width| {
        let viewport = seats::logical_viewport(
            width,
            600,
            seats::scale_ppm(1_000),
            0,
            seats::folio_band_device_px(seats::scale_ppm(1_000)),
        );
        narrow.set_focus(first);
        let with_first = narrow.solve(viewport, &metrics, SizePolicy::Lawful).ok()?;
        narrow.set_focus(second);
        let with_second = narrow.solve(viewport, &metrics, SizePolicy::Lawful).ok()?;
        (pane_rects_of(&with_first) != pane_rects_of(&with_second)).then_some(width)
    });
    let ladder = ladder.expect(
        "no window width in 200..900 lets focus change the solve — the \
             concession ladder's input has moved and this assertion is no longer \
             about what it says it is",
    );
    let revision = narrow.structure_revision();
    let ladder_viewport = seats::logical_viewport(
        ladder,
        600,
        seats::scale_ppm(1_000),
        0,
        seats::folio_band_device_px(seats::scale_ppm(1_000)),
    );
    narrow.set_focus(first);
    let with_first = narrow
        .solve(ladder_viewport, &metrics, SizePolicy::Lawful)
        .expect("solves");
    assert!(narrow.set_focus(second), "focus moves");
    let with_second = narrow
        .solve(ladder_viewport, &metrics, SizePolicy::Lawful)
        .expect("solves");
    assert_ne!(
        pane_rects_of(&with_first),
        pane_rects_of(&with_second),
        "at {ladder}px the ladder answers differently for each focus"
    );
    assert_eq!(
        narrow.structure_revision(),
        revision,
        "and clicking into a pane must not put a fifth of a second of glide \
             between the click and the layout settling"
    );

    // The tear-out's own commit is `close_seat`, which is structural.
    let before_close = narrow.structure_revision();
    assert!(narrow.close_seat(&metrics, second));
    assert_eq!(narrow.structure_revision(), before_close + 1);
    let _ = arriving;
}

/// PIN — U8, R2. A flight in progress issues **zero** ConPTY resizes.
///
/// This is the pin that keeps a resize storm impossible, and it is a pin
/// about an extent rather than about a call count: the grid is a function of
/// the seat's width and height, `coalesce_pty_resize_on_grid_change` is the
/// single gate every solve funnels through, and it schedules nothing when
/// the grid it is handed equals the one in force. So the whole of R2 is
/// "the animation never changes an extent", and that is what is asserted at
/// every frame of the flight below.
///
/// B14's counter-scale is why it can be true at all: the contents are laid
/// out at their destination size on the animation's *first* frame, so there
/// is nothing left to resize on the last. Transcribe the CSS literally —
/// scale the viewport — and the extent changes on every one of the twelve
/// frames here, each one a grid change, each one a scheduled resize behind
/// a 200ms quiet window that the next frame immediately re-arms.
#[test]
fn an_in_flight_pane_animation_asks_conpty_for_no_resize_at_all() {
    let now = Instant::now();
    let metrics = seats::seat_metrics(1_000);
    let viewport = seats::logical_viewport(
        1600,
        900,
        seats::scale_ppm(1_000),
        0,
        seats::folio_band_device_px(seats::scale_ppm(1_000)),
    );
    let (mut seats, _, split, survivor, arriving) = split_window(true);
    assert!(seats.close_seat(&metrics, arriving));
    let after = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("the survivor solves");
    let mut motion = PaneMotion::default();
    motion.begin(
        &pane_rects_of(&split),
        &pane_rects_of(&after),
        now,
        Motion::Full,
    );
    let pane = pane_box_of(&after, survivor);
    let body =
        seats::pane_body_viewport(&seats, &after, survivor, 1.0).expect("the survivor has a body");

    // The one resize the *commit* is entitled to, on the final rectangle,
    // before a single frame of the flight is drawn.
    let rows_for = |height: u32| ((height.saturating_sub(16)) / 20).max(1) as u16;
    let settled = grid_of(100, rows_for(body.height));
    let mut pending = None;
    let mut current = grid_of(100, rows_for(1));
    assert!(
        coalesce_pty_resize_on_grid_change(
            &mut pending,
            settled,
            current,
            current,
            PhysicalSize::new(body.width, body.height),
            now,
        ),
        "the commit itself schedules one resize, on the box the pane landed in"
    );
    current = take_due_pty_resize(&mut pending, now + WINDOW_RESIZE_QUIET)
        .expect("and it is delivered")
        .grid;
    assert_eq!(current, settled);

    let mut scheduled = 0_u32;
    let mut at = now;
    loop {
        let moving = motion.is_animating(at, Motion::Full);
        let (content, _) =
            animated_pane_viewports(body, pane, motion.transform_of(survivor, at, Motion::Full));
        assert_eq!(
            (content.width, content.height),
            (body.width, body.height),
            "the flight changed an extent, which is a grid change wearing an \
                 animation's clothes"
        );
        if coalesce_pty_resize_on_grid_change(
            &mut pending,
            grid_of(100, rows_for(content.height)),
            current,
            // The flight moves an extent, never a grid: the actor is wearing `current` at
            // every frame of it, which is exactly why nothing is owed and nothing is queued.
            current,
            PhysicalSize::new(content.width, content.height),
            at,
        ) {
            scheduled += 1;
        }
        if !moving {
            break;
        }
        at += pace::DEFAULT_FRAME_INTERVAL;
    }
    assert_eq!(
        scheduled, 0,
        "an in-flight animation scheduled {scheduled} ConPTY resizes; the \
             contract is exactly one per structural edit, at the commit, on the \
             final rectangle"
    );
    assert!(
        take_due_pty_resize(&mut pending, at + WINDOW_RESIZE_QUIET).is_none(),
        "and nothing is left queued behind the quiet window either"
    );
}

/// PIN — U8. A preview seat's picture travels with its pane instead of
/// waiting at the destination.
///
/// The defect this stands against is one commit deep and visible on screen:
/// `refresh_preview_for_layout` runs once per commit and used to hand the
/// renderer the *solved* body, so a preview pane's head and body glided over
/// 200ms while the picture inside them was already at the box it was going
/// to. The fix is the treatment seam 2 already got and no other — the same
/// [`animated_pane_viewports`] pair, from the same sampler, applied to one
/// more seat.
///
/// The scenario is a three-pane tab losing its leftmost pane, which is what
/// actually moves a preview: the preview's own *arrival* fades rather than
/// FLIPs (mock-up 6573), and a fade has the identity transform, so an
/// arriving preview could not tell the fixed code from the broken code. What
/// moves it is any later structural edit, and closing a pane is the shortest
/// one to write.
///
/// Three clauses, and the third is the one a naive fix loses: the picture is
/// **fitted** to the final body throughout. Fit it to the animating box
/// instead and every frame of the flight is a new `preview_image_extent`, a
/// new Lanczos target and a scale task per frame — R2's resize storm wearing
/// the image pipeline's clothes.
#[test]
fn a_preview_seats_picture_travels_with_its_pane_rather_than_waiting_at_the_destination() {
    let now = Instant::now();
    let metrics = seats::seat_metrics(1_000);
    let viewport = seats::logical_viewport(
        1600,
        900,
        seats::scale_ppm(1_000),
        0,
        seats::folio_band_device_px(seats::scale_ppm(1_000)),
    );
    let mut seats = seats::Seats::lone_terminal();
    let first = seats.identity();
    let second = seats
        .split_terminal(&metrics, first, bt_layout::Axis::Row, false)
        .expect("a 1600x900 window divides");
    seats.add_preview(&metrics).expect("the preview lands");
    let preview = seats.preview().expect("and it is in the tree");
    let before = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("three panes solve");
    assert!(
        seats.close_seat(&metrics, first),
        "the leftmost pane closes"
    );
    let after = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("two panes solve");

    let was = pane_box_of(&before, preview);
    let is = pane_box_of(&after, preview);
    assert!(
        (was[0] - is[0]).abs() > 1.0,
        "the preview's own corner has to travel, or this pin proves nothing: \
             {was:?} -> {is:?}"
    );

    let mut motion = PaneMotion::default();
    motion.begin(
        &pane_rects_of(&before),
        &pane_rects_of(&after),
        now,
        Motion::Full,
    );

    // The terminal beside it is still in the box it had, which is the frame
    // the picture has to be found in.
    let survivor = motion
        .transform_of(second, now, Motion::Full)
        .applied_to(pane_box_of(&after, second));
    for channel in 0..4 {
        assert!(
            (survivor[channel] - pane_box_of(&before, second)[channel]).abs() < 1e-3,
            "on the first frame the terminal is still where it was: {survivor:?} \
                 against {:?}",
            pane_box_of(&before, second)
        );
    }

    let opening = preview_image_placement(
        &seats,
        &after,
        preview,
        1.0,
        motion.transform_of(preview, now, Motion::Full),
    )
    .expect("the preview has a body");
    assert_ne!(
        opening.seat, opening.body,
        "the picture is not at the corner the solver just gave it"
    );
    assert_eq!(
        opening.seat.x as f32, was[0],
        "it is at the corner its own tween gives it, which is the one its head \
             is drawn at this frame"
    );
    assert_eq!(
        (opening.seat.width, opening.seat.height),
        (opening.body.width, opening.body.height),
        "and it is fitted to the final body from the very first frame — one \
             resample per commit, never one per frame"
    );
    assert!(
        opening.clip.width < opening.seat.width || opening.clip.x > opening.seat.x,
        "while the box it may appear in is still the narrow one it had: {:?} \
             against {:?}",
        opening.clip,
        opening.seat
    );

    let landed = preview_image_placement(
        &seats,
        &after,
        preview,
        1.0,
        motion.transform_of(preview, now + PANE_FLIP, Motion::Full),
    )
    .expect("the preview has a body");
    assert_eq!(
        (landed.seat, landed.clip),
        (landed.body, landed.body),
        "and both converge on the solver's answer"
    );
}

/// RED GATE — §7.1.6k⁵/§7.1.6k⁷. **Every preview pane holding a picture
/// draws it, and each is placed against its own pane** (user reports
/// 2026-09-06, three of them).
///
/// Two defects one slice apart, and one fixture separates both. The frame's
/// re-place asked [`seats::Seats::preview`] — *the first preview leaf in the
/// tree* — from the day U8 wrote it, which was the right seat for exactly as
/// long as a tab could hold one preview pane; slice 5's pin ended that, and
/// the reader photographed a picture painted on its neighbour's rectangle.
/// The mend elected one pane to a one-slot texture lane, and the reader then
/// photographed the *other* half of the same scarcity: a recording pane
/// dropped in beside a picture pane, the picture gone, "1870 × 1122 · PNG ·
/// 381 KB · Fit" over nothing.
///
/// So the fixture is two preview panes **both holding a picture** — the
/// user's own arrangement, and the minimum that can tell "the first leaf",
/// "the elected one" and "each its own" apart — and the claims are that the
/// tab names both of them and that each one's rectangle is its own pane's.
///
/// MUTATIONS:
/// ① keep only the first entry of [`TabState::seat_pictures`] (the one-slot
///    election this replaced) — the count assertion goes red at 1 against 2,
///    which is the user's photograph;
/// ② place against `seats.preview()` instead of against the surface being
///    walked — modelled here by the neighbour's placement — and the second
///    pane's left-edge assertion goes red with the first pane's corner.
#[test]
fn every_picture_is_placed_against_the_pane_that_holds_it() {
    let metrics = seats::seat_metrics(1_000);
    let viewport = seats::logical_viewport(
        1600,
        900,
        seats::scale_ppm(1_000),
        0,
        seats::folio_band_device_px(seats::scale_ppm(1_000)),
    );
    let mut seats = seats::Seats::lone_terminal();
    let first = seats.add_preview(&metrics).expect("the preview lands");
    // Locked, so the next one is a second leaf beside it rather than a reuse
    // of this one — which is the only way a tab comes to hold two (P95).
    assert!(seats.toggle_preview_lock(first), "the lock turns over");
    let second = seats.add_preview(&metrics).expect("a second preview lands");
    assert_ne!(
        first, second,
        "or the fixture is one pane wearing two names"
    );
    assert_eq!(
        seats.preview(),
        Some(first),
        "`preview()` is the first preview leaf in the tree, which is exactly \
             the wrong answer this gate is about"
    );
    let layout = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("three panes solve");

    // A picture on each: `figure.png` on one pane and the recording the
    // reader dropped beside it on the other. A video's still goes down this
    // very channel ([`Runtime::refit_preview_picture`]), which is why the
    // arrival of one could take the other's pixels away.
    let tab = TabId(7);
    let mut panes = PreviewPanes::default();
    panes
        .entry(PreviewSurface::Seat(LeafId { tab, seat: first }))
        .image = Some(PreviewImageState::new(PathBuf::from(r"D:\Demo\figure.png")));
    panes
        .entry(PreviewSurface::Seat(LeafId { tab, seat: second }))
        .image = Some(PreviewImageState::new(PathBuf::from(r"D:\Demo\clip.mp4")));
    let focused = seats.identity();
    let state = assemble_tab_state(
        tab,
        BTreeMap::from([(focused, leaf_saying("SHELL"))]),
        BTreeMap::new(),
        preview::PreviewPool::default(),
        panes,
        BTreeMap::new(),
        focused,
        TabSeed::default(),
        seats,
        layout.clone(),
        None,
    );

    let drawn = state.seat_pictures();
    assert_eq!(
        drawn,
        vec![
            PreviewSurface::Seat(LeafId { tab, seat: first }),
            PreviewSurface::Seat(LeafId { tab, seat: second }),
        ],
        "both panes are holding a picture, so both are drawing one — a tab \
             that answered with one of them is the user's blank pane"
    );

    for surface in drawn {
        let PreviewSurface::Seat(leaf) = surface else {
            unreachable!("seat_pictures answers with seats");
        };
        let placement = preview_image_placement(
            &state.seats,
            &layout,
            leaf.seat,
            1.0,
            PaneTransform::IDENTITY,
        )
        .expect("a pane holding a picture has a body");
        assert_eq!(
            placement.seat.x,
            pane_box_of(&layout, leaf.seat)[0] as u32,
            "the pixels land on the left edge of the pane that is holding \
                 them, and not on a neighbour's: {placement:?}"
        );
    }
    assert_ne!(
        pane_box_of(&layout, first)[0],
        pane_box_of(&layout, second)[0],
        "or the two panes are in the same place and nothing here is decidable"
    );
}

/// PIN — C155/C36. A press writes the durable state, and the right half of
/// it: a directory opens and a file only selects.
#[test]
fn a_press_selects_and_only_a_directory_also_opens() {
    let mut state = seats::FilesLeafState {
        root: "D:\\work".to_owned(),
        ..seats::FilesLeafState::default()
    };

    assert!(press_files_node(
        &mut state,
        "/src",
        files::RowKind::Directory { open: false }
    ));
    assert_eq!(state.sel.as_deref(), Some("/src"));
    assert!(state.open.contains("/src"));

    assert!(!press_files_node(
        &mut state,
        "/src",
        files::RowKind::Directory { open: true }
    ));
    assert!(!state.open.contains("/src"), "the second press folds it");
    assert_eq!(state.sel.as_deref(), Some("/src"), "and it stays selected");

    assert!(!press_files_node(
        &mut state,
        "/a.txt",
        files::RowKind::File
    ));
    assert_eq!(state.sel.as_deref(), Some("/a.txt"));
    assert!(
        state.open.is_empty(),
        "a file has nothing to open, and opening the tree at its path would \
             be a folder that does not exist"
    );

    assert!(!press_files_node(
        &mut state,
        "/link",
        files::RowKind::Cycle
    ));
    assert_eq!(state.sel.as_deref(), Some("/link"));
    assert!(
        state.open.is_empty(),
        "a folder that is its own ancestor refuses to open"
    );
}

/// PIN — the grid's first row is its heading, in ink and in ground.
///
/// Mutation: fill every row rather than the first, or give the data rows
/// the heading's ink.
#[test]
fn a_tables_heading_row_stands_on_its_own_fill() {
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 600.0, 400.0];
    let rows = preview::csv_rows("case,cols\ncjk,2\nemoji,2\n");
    let geometry = seats::preview_table_geometry(body, &[5, 4], rows.len(), 8.0, 1.0, [0.0, 0.0]);
    let built = build_preview_table_body(&geometry, &rows, &palette);

    let fills: Vec<_> = built
        .quads
        .iter()
        .filter(|quad| quad.color == palette.files_row_hover)
        .collect();
    assert_eq!(fills.len(), 1, "one heading fill, not one per row");
    assert_eq!(fills[0].rect, geometry.row_rect(0));
    assert!(
        built
            .quads
            .iter()
            .any(|quad| quad.color == palette.preview_grid_line),
        "and the hairlines are drawn"
    );

    let head: Vec<_> = built
        .paragraphs
        .iter()
        .filter(|p| p.runs[0].color == palette.preview_table_head_text)
        .collect();
    assert_eq!(head.len(), 2, "two heading cells");
    assert!(head.iter().all(|p| p.runs[0].bold && p.runs[0].mono));
    let data: Vec<_> = built
        .paragraphs
        .iter()
        .filter(|p| p.runs[0].color == palette.files_row_text)
        .collect();
    assert_eq!(data.len(), 4, "two data rows of two cells");
    assert!(data.iter().all(|p| !p.runs[0].bold));
}

/// PIN (user ruling, 2026-08-15) — **when the path and the phrase meet, the
/// path is what gives way.**
///
/// The path was always going to be cut: P35 gives it a left ellipsis
/// precisely because a full path rarely fits a pane's foot, and one that has
/// already lost its drive letter loses nothing new by losing three more
/// characters. The phrase has no such slack — "Read-only · 64" is not an
/// abbreviation of anything, it is a broken fact — so it is never cut, and a
/// rule that cut the shorter run to save the longer one would have had it
/// exactly backwards.
///
/// MUTATION ①: ellipsize the lead against the whole `run` instead of
/// `lead_box` and the first assertion goes red — the path is cut to a width
/// it does not have and prints straight through the phrase.
/// MUTATION ②: pass the notice through `ellipsized` as well and the
/// second goes red on a truncated truncation notice.
#[test]
fn a_long_path_is_cut_and_the_phrase_beside_it_is_not() {
    let run = [0.0, 0.0, 300.0, 28.0];
    let gap = 12.0;
    let long = r"C:\Users\somebody\Developer\folio-terminal\crates\bt-app\src\main.rs";
    let words = seats::dress_foot(
        seats::FootDress {
            dissolved: 0.0,
            run,
            lead: long,
            flash: None,
            notice: preview::preview_truncated_notice(),
            cut: seats::LeadCut::Front,
            font_px: 10.0,
            gap_px: gap,
        },
        &mut ruler,
    );
    assert!(
        ruler(&words.lead, 10.0) <= words.lead_box[2] - words.lead_box[0],
        "the path was cut to the room the phrase left it, not to the whole strip"
    );
    assert!(
        words.lead.starts_with('…') && words.lead.ends_with("main.rs"),
        "cut from the front, so the file name survives (P35): {}",
        words.lead
    );
    assert_eq!(
        words.notice,
        preview::preview_truncated_notice(),
        "and the phrase is whole"
    );

    // The same strip with nothing hung on it gives the path all of it, which
    // is what makes the loss above the phrase's doing and not the run's.
    let alone = seats::dress_foot(
        seats::FootDress {
            dissolved: 0.0,
            run,
            lead: long,
            flash: None,
            notice: "",
            cut: seats::LeadCut::Front,
            font_px: 10.0,
            gap_px: gap,
        },
        &mut ruler,
    );
    assert_eq!(alone.lead_box, run, "no phrase, no toll");
    assert!(
        alone.lead.chars().count() > words.lead.chars().count(),
        "and more of the path survives"
    );
}

/// PIN (user ruling, 2026-08-15) — **while the foot is flashing, its right
/// hand is empty.**
///
/// "Revealed in File Explorer" and "Saved" are answers to something the user
/// just did, and they stand for 1300ms before the path comes back. A
/// standing fact printed beside a confirmation turns one unambiguous word
/// into two things to read at the one moment the strip has to be read at a
/// glance — so the phrase steps aside for as long as the word stands, and
/// comes back on its own when the word expires.
///
/// The pairing is real rather than hypothetical: a truncated buffer is
/// read-only and can never flash "Saved", but its foot is still a button,
/// and pressing it flashes "Revealed" over a file whose read-only phrase is
/// standing right there.
///
/// MUTATION: drop the `flashing` guard in `dress_foot` and both the empty
/// assertion and the full-width assertion go red at once — the confirmation
/// shares its strip with a warning.
#[test]
fn a_flashing_foot_gives_the_whole_strip_to_the_word_it_is_flashing() {
    let run = [0.0, 0.0, 300.0, 28.0];
    for flash in [foot_revealed_label(), preview::preview_saved_notice()] {
        let words = seats::dress_foot(
            seats::FootDress {
                dissolved: 0.0,
                run,
                lead: r"C:\w\huge.txt",
                flash: Some(flash),
                notice: preview::preview_truncated_notice(),
                cut: seats::LeadCut::Front,
                font_px: 10.0,
                gap_px: 12.0,
            },
            &mut ruler,
        );
        assert!(words.flashing, "{flash}: the strip is confirming");
        assert_eq!(
            words.lead, flash,
            "{flash}: and says so instead of the path"
        );
        assert_eq!(words.notice, "", "{flash}: the phrase steps aside");
        assert_eq!(words.notice_width, 0.0);
        assert_eq!(
            words.lead_box, run,
            "{flash}: and pays no toll for a phrase that is not there"
        );
    }
}

/// PIN (user ruling, 2026-08-13) — **the text surface folds and has no
/// sideways; the diff keeps `pre` and has one.**
///
/// The two halves of one ruling, asserted together because the ruling is
/// about the difference. A preview is a quick look and content reachable
/// only by a hidden gesture reads as content that is not there — so the text
/// body wraps and its horizontal extent collapses to nothing, because there
/// is nowhere sideways to go. A patch is the exception and its own reason:
/// the alignment between the two columns and the full-width tint under every
/// row are what a patch *means*, and reflow destroys both.
///
/// MUTATION ①: answer `buffer.max_columns` for a wrapping text body in
/// `preview_content_extent` and the "nowhere sideways" assertion goes red —
/// the pane grows a scrollbar for a document that has no width.
/// MUTATION ②: hand the diff a `WrapLayout::wrapped` and the last assertion
/// goes red, which is a patch losing its columns.
#[test]
fn the_text_body_folds_with_no_sideways_and_the_diff_keeps_both() {
    // Deliberately shorter than the folded document, so "can be scrolled to
    // its own last row" is a question with an answer.
    let body = [0.0, 0.0, 240.0, 60.0];
    let scale = 1.0;
    let advance = 8.0;
    let metrics = seats::preview_text_metrics(scale);
    let columns = preview_wrap_columns(body, metrics, advance)
        .expect("a 240px pane at 8px a cell holds cells");
    assert!(columns > 4, "the fixture pane is wide enough to be a pane");

    // One line far wider than the pane, and one that fits.
    let long = "x".repeat(columns * 4);
    let lines = vec![long.clone(), "short".to_owned()];
    let wrap = preview_edit::WrapLayout::wrapped(&lines, columns);
    assert_eq!(
        wrap.rows(),
        5,
        "four rows for the long line and one for the short"
    );

    // ① The vertical extent is the *rows*, so the scroller can reach the end
    //    of a folded document; the horizontal one is nothing at all.
    let document = PreviewDocument::Text {
        lines: lines.clone(),
        wrap: wrap.clone(),
        highlight: highlight::Highlighting::default(),
    };
    // The file's own width is handed in, exactly as the runtime hands it in:
    // the ruling that a folded body cannot reach it is the function's, not
    // the caller's, and a caller that pre-zeroed it would prove nothing.
    let max = preview_document_max_scroll(
        &document,
        body,
        scale,
        advance,
        metrics.line_height * wrap.rows() as f32,
        long.chars().count(),
    );
    assert_eq!(max[0], 0.0, "a folded body has nowhere sideways to go");
    assert!(max[1] > 0.0, "and can be scrolled to its own last row");

    // ② The painter draws one paragraph per *row*, and the rows put the
    //    whole line back together with nothing lost off the right edge.
    let palette = bt_render::chrome_palette();
    // A pane tall enough to hold every row at once: the painter culls what
    // is off screen, and this assertion is about the folding, not the cull.
    let tall = [body[0], body[1], body[2], 400.0];
    let geometry = seats::preview_mono_geometry(
        tall,
        metrics,
        metrics.line_height * wrap.rows() as f32,
        0,
        advance,
        [0.0, 0.0],
    );
    let built = build_preview_text_body(
        &geometry,
        &lines,
        &wrap,
        &highlight::Highlighting::default(),
        advance,
        None,
        &palette,
    );
    // A row is however many runs its highlighting needs (#49) — one on this
    // fixture, several on a highlighted one — so a row's *text* is its runs
    // joined, never its first run.
    let drawn: Vec<String> = built
        .paragraphs
        .iter()
        .map(|p| p.runs.iter().map(|run| run.text.as_str()).collect())
        .collect();
    assert_eq!(drawn.len(), 5, "five rows drawn, not two lines");
    assert_eq!(
        drawn[..4].concat(),
        long,
        "and the four rows of the long line are the long line, entire"
    );
    assert!(
        drawn
            .iter()
            .take(4)
            .all(|row| row.chars().count() <= columns),
        "no row is wider than the pane it folded into"
    );
    // Consecutive rows sit one line height apart — a folded line's second
    // row is a row, not an overprint of its first.
    for pair in built.paragraphs.windows(2) {
        assert_eq!(pair[1].rect[1] - pair[0].rect[1], geometry.line_height);
    }

    // The diff is the exception, on both counts.
    let rows = vec![DiffRow {
        text: long.clone(),
        kind: preview::DiffLineKind::Add,
        top: 0.0,
    }];
    let diff = PreviewDocument::Diff(rows);
    let diff_max = preview_document_max_scroll(
        &diff,
        body,
        scale,
        advance,
        seats::preview_diff_metrics(scale).line_height,
        long.chars().count(),
    );
    assert!(
        diff_max[0] > 0.0,
        "a patch does not reflow, so it keeps the horizontal scroll it needs"
    );
}

/// PIN — the selection's band is drawn **under** the text, in the same body,
/// and the caret stands in it.
///
/// The band has to ride in [`bt_render::PreviewBody::quads`] rather than go
/// out as seat chrome: chrome is drawn a whole pass earlier and would sit
/// under the *pane*, so a band painted there would stay put while the
/// document it is about scrolled away from it.
///
/// Mutation: derive the band's right edge from the paragraph's own text
/// instead of from the columns, which drops the break at the end of every
/// line inside a multi-line selection.
#[test]
fn a_selection_band_is_drawn_under_the_text_of_its_own_body() {
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 400.0, 200.0];
    let metrics = seats::preview_text_metrics(1.0);
    let lines = vec!["one".to_owned(), "two".to_owned(), "three".to_owned()];
    let geometry =
        seats::preview_mono_geometry(body, metrics, metrics.line_height * 3.0, 5, 8.0, [0.0, 0.0]);
    let paint = PreviewEditPaint {
        bands: vec![(0, 1, 4), (1, 0, 4)],
        caret: Some((1, 3)),
        caret_width: 1.0,
        preedit: None,
    };
    let wrap = preview_edit::WrapLayout::unwrapped(&lines);
    let built = build_preview_text_body(
        &geometry,
        &lines,
        &wrap,
        &highlight::Highlighting::default(),
        8.0,
        Some(&paint),
        &palette,
    );
    assert_eq!(built.quads.len(), 3, "two bands and one caret");
    assert_eq!(built.quads[0].color, palette.preview_selection);
    let row = geometry.line_rect(0);
    assert_eq!(
        built.quads[0].rect,
        [row[0] + 8.0, row[1], row[0] + 32.0, row[3]]
    );
    // The second line's band runs one column past its own three characters:
    // the break is selected too.
    let second = geometry.line_rect(1);
    assert_eq!(built.quads[1].rect[2], second[0] + 32.0);
    let caret = built.quads[2];
    assert_eq!(caret.color, palette.preview_caret);
    assert_eq!(
        caret.rect,
        [second[0] + 24.0, second[1], second[0] + 25.0, second[3]]
    );
    // And the text is still there, over the top of all three.
    assert_eq!(built.paragraphs.len(), 3);
    assert_eq!(built.paragraphs[0].runs[0].text, "one");

    // With no edit surface there are no quads at all, which is the read-only
    // body slice 2 shipped.
    let plain = build_preview_text_body(
        &geometry,
        &lines,
        &wrap,
        &highlight::Highlighting::default(),
        8.0,
        None,
        &palette,
    );
    assert!(plain.quads.is_empty());
}

/// PIN — the triangle is not touched by the ruling above: one press folds,
/// and a row with no second verb still breaks the chain behind it.
#[test]
fn one_press_on_a_folder_row_still_only_turns_its_triangle() {
    let mut state = seats::FilesLeafState {
        root: r"D:\work".to_owned(),
        ..seats::FilesLeafState::default()
    };
    assert!(press_files_node(
        &mut state,
        "/src",
        files::RowKind::Directory { open: false }
    ));
    assert!(state.open.contains("/src"));
    assert_eq!(state.root, r"D:\work", "a first press moves no root");
    assert!(!press_files_node(
        &mut state,
        "/src",
        files::RowKind::Directory { open: true }
    ));
    assert!(state.open.is_empty());
    assert_eq!(state.root, r"D:\work");

    // Both kinds of node are counted now — the folder row has a second
    // meaning of its own, so it can no longer be the thing that breaks a
    // file row's pair. The rows that carry no verb still are.
    assert!(files_row_counts_clicks(files::RowKind::File));
    assert!(files_row_counts_clicks(files::RowKind::Directory {
        open: false
    }));
    assert!(!files_row_counts_clicks(files::RowKind::Cycle));
    assert!(!files_row_counts_clicks(files::RowKind::Notice(
        files::RowNotice::Empty
    )));
}

/// RED — **a preview wears no strip at all, and the two derivations of where
/// its body stands agree about that** (user ruling 2026-08-29, reversed for
/// previews by the owner's ruling 2026-09-12; §7.1.3x ①②).
///
/// The band was written for a terminal and widened to previews, and a
/// document then wore both it and a rail — so the order of the two
/// subtractions, which had been a tie-break nobody could see, became visible.
/// The owner's ruling takes the band off the preview outright: news floats
/// over the document as a pill and costs the layout nothing, so a preview's
/// body is its pane less its head and its rail, and nothing else.
///
/// What the original claim was really holding is kept whole and is what this
/// still tests: `seats::preview_body_viewport` and
/// `seats::preview_pane_geometry` are two derivations of one number, and a
/// pair that disagreed by the height of a band would fit a picture over
/// somebody's sentence and put every click one row away from what it looks
/// like it is on.
///
/// RED GATE: make a preview wear a notice again — from `settle_pane_notices`
/// or by hand, as here — and the body moves under it while the pill that
/// carries the news is drawn somewhere else entirely. RED GATE ②: reserve a
/// band in `preview_pane_geometry` and the two derivations part company.
#[test]
fn a_preview_wearing_a_strip_gives_it_a_row_of_its_own_body() {
    let mut seats = seats::Seats::lone_terminal();
    let seat = seats
        .add_preview(&cross_metrics())
        .expect("the preview seat lands");
    let _ = seats.set_rails(BTreeMap::from([(seat, seats::PreviewRailKind::Crumbs)]));
    let (layout, _) = cross_solve(&seats);
    let scale = 1.0;
    let rect = seats::full_pane_rect(&layout, seat).expect("a full pane");

    let body = seats::preview_body_viewport(&seats, &layout, seat, scale).expect("a body");
    let geometry = seats::preview_pane_geometry(rect, scale, seats.seat_rail(seat));
    assert_eq!(
        geometry.body[1] as u32, body.y,
        "the rectangle derivation and the viewport one do not say the same number"
    );
    assert_eq!(
        geometry.body[3], rect[3],
        "a preview's document stops short of the pane's own floor"
    );
    assert_eq!(
        geometry.body[1],
        seats::preview_rail_band(rect, scale)[3],
        "something other than the path row is being taken off the top of the document"
    );
    // And the news it may owe is cut out of that body rather than off it.
    let pill =
        seats::news_pill_box(geometry.body, scale, 120.0).expect("a pane this size holds one");
    assert!(pill[1] > geometry.body[1] && pill[3] < geometry.body[3]);
    // The band is the terminal's alone now: a preview seat that somehow wore
    // one would be a row standing on a document whose news is elsewhere.
    assert!(
        !seats.seat_wears_notice(seat),
        "a preview seat wears a band"
    );
}

/// **T5 — the shell of a `[files | shell]` tab exits and the folder stays.**
///
/// The ruling's "关最后一个座位 = 关 tab" from the other side: closing the
/// last *shell* is not closing the tab, and what is left has to be a whole
/// tab rather than a wreck. Four things are asked of it, and they are the
/// four a reader downstream would otherwise hit in turn — the identity seat
/// moved to a seat that still exists, `focused_leaf` stopped naming the seat
/// that left, the tab is named by the column it now is, and it wears no dot.
///
/// The three steps are exactly `close_pane`'s, in `close_pane`'s order; the
/// branch that decides *whether* to take them instead of closing the tab is
/// [`closing_this_pane_closes_the_tab`], pinned one test above.
///
/// Red gate: leave `refocus_after_losing` answering only when a session
/// survives and `focused_leaf` goes on naming the departed seat — a dangling
/// id that the first reader to forget the `focused()` guard turns into a
/// panic; leave `close_seat` repointing `identity` at `terminals().first()`
/// and the tab is named by a seat its tree does not have.
#[test]
fn the_last_shell_of_a_files_and_shell_tab_leaves_a_whole_folder_tab_behind() {
    let mut tab = tab_with_a_files_column(1, "D:\\work\\folio");
    let column = tab.seats.files()[0];
    let shell = tab.seats.terminals()[0];
    assert_eq!(tab.focused_leaf, shell, "the shell holds the keyboard");
    assert!(
        !closing_this_pane_closes_the_tab(tab.seats.pane_count()),
        "two panes, so this is an ordinary pane close"
    );

    assert!(tab.seats.close_seat(&cross_metrics(), shell));
    tab.sessions.remove(&shell);
    tab.refocus_after_losing(shell);

    assert!(tab.sessions.is_empty(), "the shell is gone");
    assert_eq!(tab.seats.pane_count(), 1, "and the column is not");
    assert_eq!(
        tab.seats.identity(),
        column,
        "the tab is now identified by the column, which its tree does have"
    );
    assert_eq!(
        tab.focused_leaf, column,
        "and the keyboard's seat is a seat rather than a hole"
    );
    assert!(
        tab.focused().is_none(),
        "there is still nothing to type into, which is the honest reading"
    );
    assert_eq!(tab.display_title(), "folio");
    assert_eq!(tab.tab_mark(&BTreeMap::new()), marks::ChromeMark::Folder);
    assert_eq!(
        tab.mark_state(
            true,
            Instant::now(),
            Motion::Full,
            &bt_render::chrome_palette()
        )
        .dot,
        None,
        "and no ledger left to report a dot from"
    );
    assert!(tab.sessions_match_terminals());
    assert!(tab.files_match_files_seats());
}

/// PIN — U12. **Every terminal leaf reaches the seat list, every frame.**
///
/// `redraw` builds its `SeatFrame` slice by walking `seats.terminals()`
/// through two `filter_map`/`let else` gates — a body viewport, a device
/// rectangle, and a session filed under that seat — and a seat that fails
/// any of the three is *silently skipped*. A skipped seat is a pane that is
/// simply never drawn: no error, no log, a hole in the window. Cheap to
/// reintroduce and, as the blank-second-pane bug showed, expensive to find
/// from a screenshot, because a pane that is never drawn and a pane that is
/// painted over look exactly alike.
///
/// Pinned on the three gates rather than on the slice itself, because the
/// slice needs a GPU and these do not — and the gates are what a regression
/// would break. `cross_tab` builds its tree with the same `SplitSeat` edit
/// the split verb runs, so these are a real tab's ids and ratios.
#[test]
fn every_terminal_leaf_of_a_split_tab_can_be_drawn() {
    let scale = seats::scale_ppm(CROSS_DPI) as f32 / 1_000_000.0;
    for panes in 1..=4 {
        let words: Vec<String> = (0..panes).map(|index| format!("pane {index}")).collect();
        let texts: Vec<&str> = words.iter().map(String::as_str).collect();
        let tab = cross_tab(1, &texts);
        let terminals = tab.seats.terminals();
        assert_eq!(terminals.len(), panes, "the tree really has {panes} shells");
        assert!(
            tab.sessions_match_terminals(),
            "one shell per terminal leaf, and no leaf without one"
        );
        for seat in terminals {
            assert!(
                seats::pane_body_viewport(&tab.seats, &tab.seat_layout, seat, scale).is_some(),
                "{seat:?} of a {panes}-pane tab has no body to draw into, so \
                     `redraw` would skip it and leave a hole in the window"
            );
            assert!(
                tab.seat_layout
                    .get(seat)
                    .and_then(|placement| placement.device_rect)
                    .is_some(),
                "{seat:?} of a {panes}-pane tab has no device rectangle"
            );
        }
    }
}

/// A drag is measured from the body of the pane it began in, and stays inside
/// that pane however far the pointer travels.
///
/// Two claims, both of them the same defect seen from different sides: the
/// pane-relative point a cell is looked up with must come from *this* pane's
/// body rectangle, and a pointer that has left the pane must still name a cell
/// of it rather than one of the neighbour it wandered into.
///
/// MUTATION: hand `clamp_into_body` the primary seat's body instead of the
/// pane's own — the basis this bug was made of — and the first assertion goes
/// red, because the same screen point then answers a different pane-relative
/// column entirely (the third assertion measures exactly how far off it is).
#[test]
fn a_drag_is_measured_from_its_own_panes_body_and_stays_inside_it() {
    let scale = seats::scale_ppm(CROSS_DPI) as f32 / 1_000_000.0;
    let seats = cross_seats(2);
    let (layout, _) = cross_solve(&seats);
    let terminals = seats.terminals();
    let (first, second) = (terminals[0], terminals[1]);
    let body_of = |seat| {
        seats::pane_body_viewport(&seats, &layout, seat, scale)
            .expect("both panes of a two-pane tab are placed")
    };
    let (first_body, second_body) = (body_of(first), body_of(second));
    assert_ne!(
        first_body.x, second_body.x,
        "a row split puts the two bodies at different origins, which is the \
             whole reason the basis matters"
    );

    // A point 37px across and 41px down inside the second pane, in the window's
    // own coordinates — what `pointer_position` carries.
    let x = f64::from(second_body.x) + 37.0;
    let y = f64::from(second_body.y) + 41.0;
    assert_eq!(
        clamp_into_body(second_body, x, y),
        (37.0, 41.0),
        "the pane the drag began in measures the pointer from its own corner"
    );
    assert_eq!(
        clamp_into_body(first_body, x, y),
        (f64::from(first_body.width) - 1.0, 41.0),
        "measured from the primary pane's body the same point is not 37px in \
             at all — it is past that pane's last column, which is the cell the \
             old basis would have selected"
    );

    // The pointer crosses back into the first pane mid-drag: the selection is
    // clamped to the origin pane's near edge and never reaches into the
    // neighbour's cells.
    let back_x = f64::from(first_body.x) + 5.0;
    let back_y = f64::from(first_body.y) + 5.0;
    assert_eq!(
        clamp_into_body(second_body, back_x, back_y).0,
        0.0,
        "a drag dragged into the pane next door is held at its own near edge"
    );

    // Off the bottom of the window entirely: the convention is "select to the
    // end of what is there", not "stop selecting".
    assert_eq!(
        clamp_into_body(second_body, x, 100_000.0),
        (37.0, f64::from(second_body.height) - 1.0),
        "a drag past the bottom edge selects to the pane's last row"
    );
    assert_eq!(
        clamp_into_body(second_body, -100_000.0, -100_000.0),
        (0.0, 0.0),
        "and past the top-left corner, to its first cell"
    );
}

/// **§7.1.6k — the pane moves into the tab you let go on, appended at the end
/// of its tree, and the shell goes with it.**
///
/// The ruling: *"直接松在 tab 上 = 移入该 tab(追加为树末尾分屏)"*. Three
/// things have to be true of that and are asserted against something only the
/// travelling session knows — the word on its own screen — rather than
/// against a count any freshly spawned shell would also satisfy:
///
/// * it is the **same** `DualPlaneSession`, not a respawn;
/// * it is at the **end** of the target's tree, on the trailing side;
/// * the tab it left is a whole tab, one pane lighter, with its own shells
///   untouched and item 6 holding on both sides.
///
/// Red gate: spawn into the target instead of moving and `BETA` is gone from
/// the window; aim the rim at `Left` and the arriving pane leads the tree it
/// was appended to; leave the source's tree alone and the pane is in two tabs
/// at once.
#[test]
fn a_pane_let_go_on_another_tab_arrives_at_the_end_of_its_tree_with_its_shell() {
    let mut from = cross_tab(1, &["ALPHA", "BETA"]);
    let mut into = cross_tab(2, &["GAMMA"]);
    let travelling = from.seats.terminals()[1];

    let moved = cross_move(
        &mut from,
        &mut into,
        travelling,
        seats::DropEdge::Right,
        false,
    )
    .expect("a two-pane tab may give one away, and a one-pane tab may take it");
    assert!(
        !moved.source_emptied,
        "a pane stayed behind, so the tab it left is still a tab"
    );

    assert_eq!(
        tab_texts(&into),
        vec!["GAMMA".to_string(), "BETA".to_string()],
        "the very session that was in the right-hand pane is in the other \
             tab now, and it is the last leaf of it"
    );
    assert_eq!(
        into.seats
            .tree()
            .seats_in_order()
            .last()
            .map(|seat| seat.id),
        Some(moved.landed),
        "\"追加为树末尾\": the arriving pane is the end of the tree"
    );
    assert_eq!(
        tab_texts(&from),
        vec!["ALPHA".to_string()],
        "and the pane that stayed is untouched"
    );
    assert_eq!(from.seats.pane_count(), 1);
    assert!(
        from.sessions_match_terminals(),
        "item 6, on the tab it left"
    );
    assert!(
        into.sessions_match_terminals(),
        "item 6, on the tab it joined"
    );
    assert_eq!(
        into.seats.focus(),
        moved.landed,
        "D43: the focus goes where the promise was drawn"
    );
    assert_eq!(
        into.focused_leaf,
        SeatId(1),
        "but the keyboard does not follow a pane into a tab nobody is \
             looking at"
    );
}

/// A tab of one terminal and **two** preview panes, each on its own file.
///
/// The shape cell ① needs and [`tab_with_a_preview`] cannot make: a source
/// that still has a preview pane after one of them leaves, so the
/// whole-pool rule ("若是原 tab 最后一个预览 pane 则整池随行") does not fire
/// and the travelling pane is on its own. The lock is how a second preview
/// seat is asked for at all — `add_preview` reuses an unlocked one.
fn tab_with_two_previews(
    id: u64,
    buffers: Vec<preview::PreviewBuffer>,
) -> (TabState, SeatId, SeatId) {
    let mut seats = seats::Seats::lone_terminal();
    let first = seats
        .add_preview(&cross_metrics())
        .expect("the first preview seat lands");
    assert!(seats.toggle_preview_lock(first));
    let second = seats
        .add_preview(&cross_metrics())
        .expect("a locked pane is not a reuse target");
    let focused = seats.identity();
    let mut pool = preview::PreviewPool::default();
    let mut panes = PreviewPanes::default();
    for (seat, buffer) in [first, second].into_iter().zip(buffers) {
        panes.entry(seat_of(TabId(id), seat)).buffer = Some(buffer.source.clone());
        pool.insert(buffer);
    }
    let (layout, overflow) = cross_solve(&seats);
    let tab = assemble_tab_state(
        TabId(id),
        BTreeMap::from([(focused, leaf_saying("SHELL"))]),
        BTreeMap::new(),
        pool,
        panes,
        BTreeMap::new(),
        focused,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    );
    (tab, first, second)
}

/// **Cell ① — dropped on the tab list, source tab keeps its other panes.**
///
/// The plain case, and the one that isolates the fix from the whole-pool
/// rule: the source still has a preview pane of its own after this one
/// leaves, so nothing sweeps the pool across and the document has to travel
/// **with the pane that is showing it** or not at all.
///
/// Red gate: take the `preview_pool.take`/`merge_buffer` pair out of
/// [`move_seat_content`] and `document_on` answers `None` — the pane arrives
/// pointing at a source the target's pool does not hold, which on the glass
/// is the empty state.
#[test]
fn a_preview_pane_dropped_on_another_tab_arrives_showing_the_same_document() {
    let (mut from, staying, preview_seat) = tab_with_two_previews(
        1,
        vec![
            buffer_saying("D:\\work\\read.md", "read.md", "history"),
            edited_buffer("D:\\work\\notes.md", "notes.md", "hello", " and unsaved"),
        ],
    );
    from.preview_panes
        .entry(seat_of(TabId(1), preview_seat))
        .scroll = [0.0, 640.0];
    let mut into = cross_tab(2, &["GAMMA"]);

    let moved = cross_move(
        &mut from,
        &mut into,
        preview_seat,
        seats::DropEdge::Right,
        false,
    )
    .expect("a preview pane may be moved into another tab");

    assert_eq!(
        document_on(&into, moved.landed),
        Some("hello and unsaved"),
        "the pane arrives showing the document it was showing, unsaved edit \
             and all"
    );
    assert_eq!(
        into.preview_panes
            .get(into.preview_here(moved.landed))
            .expect("the arrived pane")
            .scroll,
        [0.0, 640.0],
        "and as far down it as the reader had scrolled"
    );
    assert!(
        into.preview_pool
            .get(&preview::PreviewSource::file("D:\\work\\notes.md"))
            .is_some_and(|buffer| buffer.dirty),
        "the dirty bit crossed with the bytes, so the gates that ask about \
             unsaved work ask the tab the work is now in"
    );
    assert!(
        from.preview_pool
            .get(&preview::PreviewSource::file("D:\\work\\notes.md"))
            .is_none(),
        "and it is a move: a second copy left behind is the fork the \
             one-buffer-per-file law forbids"
    );
    assert_eq!(
        document_on(&from, staying),
        Some("history"),
        "while the pane that stayed keeps its own document — the whole pool \
             did not travel, because the tab it left still has a door onto it"
    );
}

/// **Cell ③ — the stage's door, and the trade that goes the other way.**
///
/// §7.1.6k′ aims at a zone rather than at the end of the tree, and a centre
/// *trades*: the pane standing there goes back the way the traveller came.
/// Both journeys are [`move_seat_content`], so both owe their document, and
/// a fix written for the traveller alone would leave the returning pane
/// blank instead.
///
/// Red gate: carry the buffer in `pane_into_tab` beside the first
/// `move_seat_content` instead of inside it, and the second assertion goes
/// red — the traded pane crosses with nothing.
#[test]
fn a_traded_preview_pane_and_its_traveller_each_keep_their_own_document() {
    let (mut from, travelling) = tab_with_a_preview(
        1,
        vec![edited_buffer(
            "D:\\work\\notes.md",
            "notes.md",
            "hello",
            " and unsaved",
        )],
    );
    let (mut into, standing) = tab_with_a_preview(
        2,
        vec![buffer_saying("D:\\work\\other.md", "other.md", "elsewhere")],
    );

    let moved = cross_move_at(
        &mut from,
        &mut into,
        travelling,
        seats::LayoutAim::SeatCentre(standing),
        false,
    )
    .expect("a centre takes the pane and trades the one it landed on");
    let traded = moved.traded.expect("a centre trades");

    assert_eq!(
        document_on(&into, moved.landed),
        Some("hello and unsaved"),
        "the traveller arrives showing what it was showing"
    );
    assert_eq!(
        document_on(&from, traded),
        Some("elsewhere"),
        "and the pane it traded places with arrives showing what *it* was \
             showing, rather than the empty state"
    );
}

const STANDING_PATH: &str = r"D:\shots\standing.png";

/// RED ③ — **two pictures in one tab are both drawn** (user report
/// 2026-09-06, the `next38` acceptance: a recording pane dragged in from the
/// file column beside a picture pane, and the picture gone).
///
/// The rule §7.1.6k stated when it wrote `get_or_insert` — "a picture the
/// target was already showing does not lose its pixels to one that has just
/// moved in" — was as much as one texture slot could give. It bought the
/// incumbent's pixels by refusing the newcomer's, and the reader who dropped
/// the newcomer in is looking at whichever of the two lost. Both draw now
/// (§7.1.6k⁷), which is the same sentence with the scarcity taken out.
///
/// RED GATE: keep only the first entry of [`TabState::seat_pictures`] — the
/// election this replaced — and the arriving picture is missing from the
/// list, which is the user's photograph.
#[test]
fn two_pictures_in_one_tab_are_both_drawn() {
    let (mut from, travelling) = tab_with_a_picture(1, SHOT_PATH);
    let (mut into, standing) = tab_with_a_picture(2, STANDING_PATH);

    let moved = cross_move(
        &mut from,
        &mut into,
        travelling,
        seats::DropEdge::Right,
        true,
    )
    .expect("the picture pane moves into the other tab");
    assert_ne!(
        moved.landed, standing,
        "or the fixture is one pane wearing two names"
    );

    let drawn = pictures_drawn(&into);
    assert!(
        drawn.contains(&Path::new(STANDING_PATH)),
        "the incumbent keeps its pixels: {drawn:?}"
    );
    assert!(
        drawn.contains(&Path::new(SHOT_PATH)),
        "and the arriving picture is drawn too, rather than keeping its \
             head, its foot and its meta line over nothing: {drawn:?}"
    );
    assert_eq!(drawn.len(), 2, "two panes, two pictures: {drawn:?}");
}

/// **§7.1.6k — a target that cannot take another pane is refused before
/// anything moves.**
///
/// H93/M147: the survey does not light such a tab up, and the commit must
/// agree with the survey. The failure this guards is not the refusal but its
/// *timing* — plan, close the source seat, then find out — which would leave
/// the pane nowhere at all.
///
/// Red gate: move the `plan.fits()` test below the `close_seat` and the
/// source tab comes back one pane lighter with nothing to show for it.
#[test]
fn a_refused_move_leaves_both_tabs_exactly_as_they_were() {
    let mut from = cross_tab(1, &["ALPHA", "BETA"]);
    // A tree so finely divided that one more pane cannot clear MIN_PANE_W.
    let mut into = cross_tab(2, &["A", "B", "C", "D", "E", "F", "G", "H"]);
    let before_from = from.seats.tree().clone();
    let before_into = into.seats.tree().clone();
    let travelling = from.seats.terminals()[1];

    assert!(
        cross_move(
            &mut from,
            &mut into,
            travelling,
            seats::DropEdge::Right,
            false
        )
        .is_none(),
        "H93: the plan does not fit, so there is no move"
    );
    assert_eq!(*from.seats.tree(), before_from, "and nothing was taken");
    assert_eq!(*into.seats.tree(), before_into, "and nothing arrived");
    assert_eq!(
        tab_texts(&from),
        vec!["ALPHA".to_string(), "BETA".to_string()]
    );
    assert!(from.sessions_match_terminals());
    assert!(into.sessions_match_terminals());
}

// ── §7.1.6k′: the same move, aimed at the stage instead of at the tab list ─

/// **§7.1.6k′ (user ruling 2026-08-23) — a pane from another tab splits the
/// pane it was let go beside, on the side it was let go on.**
///
/// The policy §7.1.6k recorded was *"离家的 pane 在这张舞台上没有落点 … 现在是
/// 未做而不是做不了"*, and the user met it on the machine: spring across, aim
/// at the stage to choose a side, and nothing at all happens. The ruling
/// opens it, and the promise is that a foreign pane gets **the same** zones a
/// local one does — so what is asserted here is a `SeatEdge` doing exactly
/// what a `SeatEdge` does, with a shell that is the same object on both sides
/// of the move.
///
/// Red gate: keep `pane_into_tab` aiming at `Rim(edge)` however it was
/// called — which is what this file did until 2026-08-23 — and `BETA` comes
/// out beside *everything* instead of beside `GAMMA`, so the order is
/// `GAMMA, DELTA, BETA` and the first assertion fails.
#[test]
fn a_pane_from_another_tab_splits_the_one_it_was_dropped_beside() {
    let mut from = cross_tab(1, &["ALPHA", "BETA"]);
    let mut into = cross_tab(2, &["GAMMA", "DELTA"]);
    let travelling = from.seats.terminals()[1];
    let beside = into.seats.terminals()[0];

    let moved = cross_move_at(
        &mut from,
        &mut into,
        travelling,
        seats::LayoutAim::SeatEdge(beside, seats::DropEdge::Left),
        true,
    )
    .expect("a two-pane tab may give one away and a two-pane tab may split");

    assert_eq!(
        tab_texts(&into),
        vec!["BETA".to_string(), "GAMMA".to_string(), "DELTA".to_string()],
        "the side that was aimed at is the side it landed on — leading, \
             because `Left` is a leading edge — and it is beside GAMMA rather \
             than beside the whole tree"
    );
    assert_eq!(
        tab_texts(&from),
        vec!["ALPHA".to_string()],
        "and it is not in two tabs at once"
    );
    assert!(
        moved.traded.is_none(),
        "an edge cuts a new slot, so it displaces nobody"
    );
    assert!(!moved.source_emptied);
    assert!(from.sessions_match_terminals());
    assert!(into.sessions_match_terminals());
    assert_eq!(
        into.seats.focus(),
        moved.landed,
        "D43: the focus goes where the promise was drawn"
    );
    assert_eq!(
        into.focused_leaf, moved.landed,
        "and the keyboard follows it, because this is the tab on screen and \
             a shell came with it"
    );
}

/// **B4 (user ruling 2026-08-25) — a centre is a trade, and it is a trade
/// across tabs too.**
///
/// The centre used to mean two different things depending on where the pane
/// in the hand happened to live: a seat of *this* tree traded payloads
/// (`Edit::CenterSwap`, L138) while a pane from another tab replaced the
/// target and turned it out into a tab of its own (L139/N161). That was the
/// implementation's shape showing through — a foreign pane arrives as a
/// one-leaf subtree, and a subtree replaces — and the ruling ends it: **the
/// centre trades, always.** The pane that was standing there goes to the slot
/// the arriving pane vacated, which is the only place a trade can put it.
///
/// Red gate: leave the eviction in and `GAMMA` comes back as a third tab
/// instead of standing where `BETA` stood.
#[test]
fn a_foreign_pane_on_a_centre_trades_places_with_the_one_it_landed_on() {
    let mut from = cross_tab(1, &["ALPHA", "BETA"]);
    let mut into = cross_tab(2, &["GAMMA", "DELTA"]);
    let travelling = from.seats.terminals()[1];
    let target = into.seats.terminals()[0];

    let moved = cross_move_at(
        &mut from,
        &mut into,
        travelling,
        seats::LayoutAim::SeatCentre(target),
        true,
    )
    .expect("a centre displaces rather than dividing, so it always fits");

    assert_eq!(
        tab_texts(&into),
        vec!["BETA".to_string(), "DELTA".to_string()],
        "the arriving pane took the slot GAMMA stood in"
    );
    assert_eq!(
        tab_texts(&from),
        vec!["ALPHA".to_string(), "GAMMA".to_string()],
        "and GAMMA took the slot BETA vacated, carrying its own shell — a \
             trade moves two panes and turns nobody out"
    );
    assert!(
        !moved.source_emptied,
        "nothing was emptied: one pane left and one arrived"
    );
    assert!(from.sessions_match_terminals());
    assert!(into.sessions_match_terminals());
}

/// **B4's boundary — a pane that is its tab's only child trades all the same,
/// and the tab it leaves is the one the other pane arrives in.**
///
/// The ruling states it as a case because the old machine had a special one
/// here: moving the last pane out emptied the tab and took its entry out of
/// the strip (`source_emptied`). A trade has nothing to empty — the tab is
/// holding a pane the whole time, it is just a different one — so the tab
/// keeps its slot, its name and its place in the run.
///
/// Red gate: reach the trade through `close_seat` and G84 refuses to empty
/// the tree, or the tab leaves the strip with a live shell still in it.
#[test]
fn a_lone_pane_that_trades_places_leaves_its_tab_holding_the_other_one() {
    let mut from = cross_tab(1, &["ALPHA"]);
    let mut into = cross_tab(2, &["GAMMA", "DELTA"]);
    let travelling = from.seats.terminals()[0];
    let target = into.seats.terminals()[0];

    let moved = cross_move_at(
        &mut from,
        &mut into,
        travelling,
        seats::LayoutAim::SeatCentre(target),
        true,
    )
    .expect("the last pane of a tab may trade places");

    assert!(
        !moved.source_emptied,
        "the tab is not emptied — GAMMA is standing in it"
    );
    assert_eq!(
        tab_texts(&from),
        vec!["GAMMA".to_string()],
        "the pane that was displaced took over the tab the traveller left"
    );
    assert_eq!(
        tab_texts(&into),
        vec!["ALPHA".to_string(), "DELTA".to_string()],
        "and the traveller is standing where it landed"
    );
    assert!(from.sessions_match_terminals());
    assert!(into.sessions_match_terminals());
}

/// **§7.1.6k′ — the emptied source tab is still emptied, whichever zone the
/// hand opened over.**
///
/// §7.1.6k settled this for the tab list's door: *"关最后一个座位 = 关 tab"*,
/// and the tab **leaves** rather than closing, because `close_tab` would file
/// a still-running shell into Recent and shut it down. The stage's door is the
/// same move with a different aim, so it inherits the rule rather than
/// restating it — this is that claim, asserted at an edge.
///
/// Red gate: give the stage's door its own commit and this either refuses the
/// last pane (G84, `close_seat` will not empty a tree) or leaves an empty tab
/// standing in the strip, which §2.1 says is not a state that exists.
#[test]
fn a_lone_pane_dropped_on_the_stage_empties_its_tab_the_same_way() {
    let mut from = cross_tab(1, &["ALPHA"]);
    let mut into = cross_tab(2, &["GAMMA"]);
    let travelling = from.seats.terminals()[0];
    let beside = into.seats.terminals()[0];

    let moved = cross_move_at(
        &mut from,
        &mut into,
        travelling,
        seats::LayoutAim::SeatEdge(beside, seats::DropEdge::Bottom),
        true,
    )
    .expect("the last pane of a tab may still be moved");

    assert!(moved.source_emptied, "so its tab has to leave the run");
    assert!(
        from.sessions.is_empty(),
        "T226: it is leaving with no shell filed under it — the shell moved"
    );
    assert_eq!(
        tab_texts(&into),
        vec!["GAMMA".to_string(), "ALPHA".to_string()],
        "and it is running below the pane it was dropped under"
    );
}

/// **§7.1.6k′ — M147 at every aim: a stage that cannot take the pane refuses
/// it, and refuses it before anything moves.**
///
/// H93 is a fact about the layout the drop *would* make, so it is asked of
/// each aim separately — an edge and a rim both cut a new slot into a tree
/// that has no room for one. What must never happen is the refusal arriving
/// after the pluck, which would leave the pane in neither tab.
///
/// The centre is deliberately **not** in this list: a replace divides
/// nothing, so it fits where a split does not, and asserting that it refuses
/// would be pinning a bug.
///
/// Red gate: judge `fits()` after `close_seat` and the source tab comes back
/// one pane lighter with nothing to show for it.
#[test]
fn a_stage_that_will_not_take_the_pane_refuses_it_at_every_dividing_aim() {
    let crowded = ["A", "B", "C", "D", "E", "F", "G", "H"];
    for aim in [
        seats::LayoutAim::Rim(seats::DropEdge::Right),
        seats::LayoutAim::SeatEdge(SeatId(1), seats::DropEdge::Right),
    ] {
        let mut from = cross_tab(1, &["ALPHA", "BETA"]);
        let mut into = cross_tab(2, &crowded);
        let before_from = from.seats.tree().clone();
        let before_into = into.seats.tree().clone();
        let travelling = from.seats.terminals()[1];

        assert!(
            cross_move_at(&mut from, &mut into, travelling, aim, false).is_none(),
            "H93 at {aim:?}: the plan does not fit, so there is no move"
        );
        assert_eq!(*from.seats.tree(), before_from, "and nothing was taken");
        assert_eq!(*into.seats.tree(), before_into, "and nothing arrived");
        assert!(from.sessions_match_terminals());
        assert!(into.sessions_match_terminals());
    }
}

/// PIN (T-PROFILE-TABLE-MOVE) — **a pane names its profile by the one thing
/// a table move cannot touch**, so reordering Settings ▸ Profiles cannot
/// change what a split, a restart or a save says that pane is.
///
/// The bug this closes had two faces and one cause. A seat held a *position*
/// in a table every window in the process shares, and the Profiles page moves
/// positions for a living: after `Move up` on the row above it, a split
/// spawned whichever row had slid into the seat's slot (the wrong shell,
/// silently, with no banner), and the save wrote that row's id into
/// `session.json`, so the wrong answer outlived the window. After a `Delete`
/// the position could name no row at all, and the spawn read past the end of
/// the table and panicked the window thread, which is every tab in the
/// process.
///
/// There is no position left to move. Every value below is the id the leaf
/// itself holds, carried through unchanged, which is why the test can state
/// the property with an id the table does not hold at all — the strongest
/// form of "nothing here is resolved against the table" that can be written.
///
/// Red gate: put the index back on `LeafSession` and the last block cannot be
/// expressed at all — there is no `usize` that means "a row this table has
/// not got" — and the first three assertions become a statement about
/// whatever row happens to sit where `cmd` sat.
#[test]
fn a_pane_carries_its_profile_by_id_so_a_table_move_cannot_move_it() {
    let mut tab = cross_tab(1, &["ALPHA", "BETA"]);
    let [left, right] = tab.seats.terminals()[..] else {
        panic!("the fixture is a row of two terminals");
    };
    for (seat, id) in [(left, "cmd"), (right, "gitbash")] {
        tab.sessions
            .get_mut(&seat)
            .expect("the fixture files a session under every terminal")
            .profile = id.to_owned();
    }

    // What the chrome, the seeds and the save all read is the same string.
    assert_eq!(tab.leaf_profile(left), "cmd");
    assert_eq!(tab.leaf_profile(right), "gitbash");
    let seed = restart_seed(&tab.leaf_profile(left), None);
    assert_eq!(seed.profile, "cmd", "a restart is the seat's own shell");
    assert_eq!(
        SplitSeed::Inherit
            .applied(&tab.leaf_profile(right), None)
            .profile,
        "gitbash",
        "and so is `another one of these`"
    );
    assert_eq!(
        SplitSeed::Profile("wsl".to_owned())
            .applied(&tab.leaf_profile(left), None)
            .profile,
        "wsl",
        "a row the reader named is that row and not its place in the list"
    );

    // And the save writes each pane's own id rather than resolving a
    // position against the table as it stands at save time.
    assert_eq!(tab.term_leaf(left, false).profile_id, "cmd");
    assert_eq!(tab.term_leaf(right, false).profile_id, "gitbash");

    // **A row that is really gone**, which is the case a position could not
    // even express. The pane goes on running the shell it started, the save
    // keeps the name of what it was, and the revive spends the degradation
    // once — the fallback profile, with the missing id carried so the pane's
    // first line can say it.
    tab.sessions.get_mut(&left).expect("the left pane").profile = "a-row-nobody-has".to_owned();
    assert!(!profiles::has_id("a-row-nobody-has"));
    assert_eq!(tab.term_leaf(left, false).profile_id, "a-row-nobody-has");

    let saved = TabV1 {
        root: tab
            .seats
            .to_persisted(&|seat| tab.term_leaf(seat, false), &|seat| {
                tab.files_state(seat)
            }),
        pinned: false,
        focused_leaf: "leaf-0".to_owned(),
        preview: None,
    };
    let (seats, _seed, leaves, _files, _preview) = revive_plan(&saved);
    let [revived_left, revived_right] = seats.terminals()[..] else {
        panic!("two saved terminals come back as two seats");
    };
    assert_eq!(
        leaves[&revived_left].profile,
        profiles::fallback_profile_id(),
        "a profile this table has not got costs the pane its shell choice"
    );
    assert_eq!(
        leaves[&revived_left].unknown_profile_id.as_deref(),
        Some("a-row-nobody-has"),
        "and never the pane, nor the sentence that says what it was"
    );
    assert_eq!(
        leaves[&revived_right].profile, "gitbash",
        "the pane beside it is untouched by any of that"
    );
    assert_eq!(leaves[&revived_right].unknown_profile_id, None);
}

/// PIN — **the picker's `Left` and `Up` put the arriving pane first in the
/// tree**, which is the only thing about those two zones that is not free.
///
/// `Seats::split_terminal` has taken a `leading` flag since the tree existed
/// and every caller passed `false`; the picker is the first to pass `true`,
/// so what is worth pinning is that the flag reaches the tree and reverses
/// the order — read off `terminals()`, which walks the tree in order (D2).
///
/// Red gate: pass `false` for `Left` and the new pane appears on the right
/// while the diagram said left, which is a lie told in a picture.
#[test]
fn splitting_toward_the_leading_side_puts_the_new_pane_first_in_the_tree() {
    let metrics = cross_metrics();
    for zone in profiles::SplitZone::ALL {
        let mut seats = seats::Seats::lone_terminal();
        let source = seats.identity();
        let arriving = seats
            .split_terminal(&metrics, source, zone.axis(), zone.leading())
            .expect("a lone terminal has room to divide");
        let order = seats.terminals();
        let expected = if zone.leading() {
            vec![arriving, source]
        } else {
            vec![source, arriving]
        };
        assert_eq!(
            order, expected,
            "{zone:?} puts the arriving pane where the diagram showed it"
        );
    }
}

/// **N159/K124 — a tab merged into a layout hands its whole fleet over, and
/// every shell survives the crossing.**
///
/// Four distinct words, two tabs, one merge: afterwards all four are in the
/// target and each is filed under the seat the renumbering gave it. That is
/// the map [`seats::DropPlan::arrived`] exists for, and the assertion that
/// every word survived is what tells a migration apart from a respawn.
///
/// **T226 beside it**: the source is left holding nothing, which is what
/// makes removing its strip entry safe without `close_tab` — there is no
/// empty tab, because there is no tab.
///
/// Red gate: migrate under the *old* keys and item 6 goes red on the target;
/// skip the migration entirely and the target has four Terminal leaves and
/// two shells, which is I106's black rectangle twice over.
#[test]
fn a_merging_tab_hands_over_every_shell_it_held() {
    let mut source = cross_tab(1, &["SRCA", "SRCB"]);
    let mut target = cross_tab(2, &["TGTA", "TGTB"]);
    let arrived = cross_merge(
        &source.seats,
        &mut target,
        seats::LayoutAim::SeatEdge(SeatId(2), seats::DropEdge::Right),
    );
    let ejected = absorb_tab_into_layout(
        &mut source,
        &mut target,
        &arrived,
        None,
        TabId(9),
        cross_solve,
    );
    assert!(
        ejected.is_none(),
        "L136: an edge landing displaces nobody, so nothing goes back to the strip"
    );
    assert!(
        source.sessions.is_empty(),
        "T226: the merge takes the whole fleet, so no empty tab is left behind"
    );
    assert!(target.sessions_match_terminals(), "item 6");
    assert_eq!(target.seats.terminals().len(), 4);
    let mut words = tab_texts(&target);
    words.sort();
    assert_eq!(
        words,
        vec!["SRCA", "SRCB", "TGTA", "TGTB"],
        "every shell that was running is still running, in the tab that absorbed it"
    );
}

/// **D44 over D43, and N160① beside it.**
///
/// `adopt_drop` has already put focus on the first seat the accent box
/// covered (D43) — for a tab arriving at an edge that is the arriving
/// layout's *first* leaf. D44 says the merged tab keeps its **own** focused
/// leaf, so the source tab is given focus on its second pane and the target
/// must end up there, remapped through `arrived`, keyboard and layout focus
/// together. Asserting the word rather than the id is what makes this a
/// statement about the pane the user was working in.
///
/// N160① is the same gesture's other half: a pinned source makes the target
/// pinned, because the target now holds the thing you asked to have back.
///
/// Red gate: delete the D44 block and focus stays on `SRCA`, which is D43's
/// answer and the wrong one for this landing; drop the `|=` and the pin is
/// lost with the tab that carried it.
/// PIN — R3: **the top bar's hit table, both layouts, end to end.**
///
/// The two halves of the drag bug in one place: `seats` decides where the
/// app's run ends and `bt_platform` turns that into `HTCLIENT`/`HTCAPTION`,
/// and neither half alone can say whether the window can be dragged. Thirty
/// tabs, because that is the case that made a strip claim the whole bar — and
/// with the tabs in the rail there is no strip in the bar to claim it.
///
/// Red gate: hand the frame an empty list and the toggle and the gear stop
/// being the application's; hand it the whole bar as one box and the window
/// will not move — which is precisely the bug R3 was, a title bar that
/// answers `Client` everywhere.
///
/// **And the second half of it is the owner's ruling of 2026-09-13**
/// (§13.11 ⑥): the frame is no longer told where the application's run
/// *ends*, it is told the application's own boxes, so the air between two
/// tabs and the band above a control shorter than the bar drag like the rest
/// of it.
#[test]
fn the_top_bar_drags_beside_the_toggle_when_the_tabs_are_in_the_rail() {
    use bt_platform::CustomFrameHit;
    let (width, scale, tabs) = (960.0_f32, 1.0_f32, 30);
    let vertical = seats::RailState {
        layout: seats::TabLayoutMode::Vertical,
        mode: seats::RailMode::Expanded,
        ..seats::RailState::default()
    };
    let horizontal = seats::RailState::default();
    // The very list `refresh_chrome` hands the frame, built the way it
    // builds it — the boxes this window draws in its bar, on whole pixels.
    let boxes = |rail| -> Vec<[i32; 4]> {
        seats::title_bar_folio_boxes(
            width,
            scale,
            crate::seats::FOLIO_BAR,
            tabs,
            0.0,
            rail,
            false,
        )
        .into_iter()
        .map(|rect| rect.map(|edge| edge.ceil() as i32))
        .collect()
    };
    let hit = |rail, x, y| {
        let app_boxes = boxes(rail);
        bt_platform::custom_frame_hit_test(
            bt_platform::CustomFrameMetrics {
                width: width as i32,
                height: 600,
                title_bar_height: 40,
                app_boxes: &app_boxes,
                resize_border: 8,
                resizable: true,
            },
            x,
            y,
        )
    };

    // The rail's bar: one button, then handle all the way to the gear.
    assert_eq!(
        hit(vertical, 20, 20),
        CustomFrameHit::Client,
        "the toggle itself"
    );
    assert_eq!(
        hit(vertical, 20, 36),
        CustomFrameHit::Caption,
        "and the band under it, which the toggle is too short to reach \
             (above it is the window's resize edge, which outranks the bar)"
    );
    assert_eq!(
        hit(vertical, 60, 20),
        CustomFrameHit::Caption,
        "the name is inside `.drag`, so it drags"
    );
    assert_eq!(
        hit(vertical, 400, 20),
        CustomFrameHit::Caption,
        "R3: the empty middle of the bar is what the hand reaches for"
    );
    assert_eq!(
        hit(vertical, 770, 20),
        CustomFrameHit::Caption,
        "right up to the gear"
    );
    assert_eq!(
        hit(vertical, 800, 20),
        CustomFrameHit::Client,
        "the gear's own box"
    );
    assert_eq!(
        hit(vertical, 400, 60),
        CustomFrameHit::Client,
        "below the bar is the terminal's, in either layout"
    );

    // The strip's bar: every box in it is the application's and must not be
    // draggable, which is the promise the vertical fix may not break.
    let strip = boxes(horizontal);
    for rect in &strip {
        assert_eq!(
            hit(horizontal, (rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2),
            CustomFrameHit::Client,
            "a box this window draws at {rect:?} is not the window's to be \
                 dragged by — handing the tabs to the drag handler is the bug \
                 this boundary exists to prevent"
        );
    }
    // And what is left of the bar is the window's — including, since the
    // ruling, the air between two tabs.
    let held = |x: i32, y: i32| {
        strip
            .iter()
            .any(|rect| x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3])
    };
    let first_tab = strip
        .iter()
        .map(|rect| rect[0])
        .filter(|left| *left > 0)
        .min()
        .expect("a thirty-tab strip has tabs in it");
    let gap = (first_tab..700)
        .find(|x| !held(*x, 20))
        .expect("thirty tabs at their floor stand apart, and the gaps are air");
    assert_eq!(
        hit(horizontal, gap, 20),
        CustomFrameHit::Caption,
        "x={gap} is inside the strip's own run and inside none of its boxes, \
             so it is the window's"
    );
}

#[test]
fn a_merged_tab_keeps_its_own_focused_leaf_and_its_pin() {
    let mut source = cross_tab(1, &["SRCA", "SRCB"]);
    source.focused_leaf = SeatId(2);
    source.pinned = true;
    let mut target = cross_tab(2, &["TGTA", "TGTB"]);
    assert!(!target.pinned);
    let arrived = cross_merge(
        &source.seats,
        &mut target,
        seats::LayoutAim::SeatEdge(SeatId(2), seats::DropEdge::Right),
    );
    let landed_first = arrived
        .iter()
        .find(|(was, _)| *was == SeatId(1))
        .expect("the arriving tab's first seat was renamed")
        .1;
    absorb_tab_into_layout(
        &mut source,
        &mut target,
        &arrived,
        None,
        TabId(9),
        cross_solve,
    );

    assert_eq!(
        leaf_says(&target, target.focused_leaf),
        "SRCB",
        "D44: the keyboard is in the pane the merged tab was working in"
    );
    assert_eq!(
        target.seats.focus(),
        target.focused_leaf,
        "layout focus and the keyboard land on the same seat"
    );
    assert_ne!(
        target.focused_leaf, landed_first,
        "D44 overrode D43's landed-box focus rather than agreeing with it by luck"
    );
    assert!(target.pinned, "N160(1): pin follows content");
}

/// **N161/L139/K125 — the displaced pane goes back to the strip carrying its
/// own shell, and N160② decides its pin.**
///
/// The whole of the replace, minus the strip surgery only the window can do.
/// A tab is let go on the target's second pane; `ReplaceSeat` seats the
/// arriving layout where that pane stood, the arriving tab's shells migrate
/// in, and the pane that was pushed out becomes a tab of its own running the
/// shell it was already running — `TGTB`, which is in no other session in
/// this test.
///
/// **N160② is the opposite of N158 and the distinction is the ruling's.**
/// The target tab was pinned, so the pane it was forced to give up stays
/// pinned: it was living under that pin, and being displaced by a drop aimed
/// somewhere else is not you changing your mind about wanting it back.
/// N158's pane was aimed at the strip by hand and starts unpinned.
///
/// Red gate: capture the displaced seat *after* the adoption and there is
/// nothing to capture, so nothing is ejected and `TGTB`'s shell is dropped on
/// the floor with its ConPTY still running; hand the ejected tab `false` and
/// N160② goes red.
#[test]
fn a_replaced_pane_is_ejected_to_the_strip_with_its_shell_and_its_pin() {
    let mut source = cross_tab(1, &["SRCA", "SRCB"]);
    let mut target = cross_tab(2, &["TGTA", "TGTB"]);
    target.pinned = true;
    let displaced = target
        .seats
        .tree()
        .find_seat(SeatId(2))
        .cloned()
        .expect("the target pane is in the target tree before the adoption");
    let arrived = cross_merge(
        &source.seats,
        &mut target,
        seats::LayoutAim::SeatCentre(SeatId(2)),
    );
    assert!(
        !target.seats.tree().contains(SeatId(2)),
        "ReplaceSeat took the target pane out of the tree"
    );
    let ejected = absorb_tab_into_layout(
        &mut source,
        &mut target,
        &arrived,
        Some(&displaced),
        TabId(9),
        cross_solve,
    )
    .expect("the displaced pane had a session to carry");

    assert_eq!(
        leaf_says(&ejected, SeatId(1)),
        "TGTB",
        "the ejected pane is running the shell it was already running"
    );
    assert!(
        ejected.pinned,
        "N160(2): it was living under the target tab's pin and was displaced, not aimed"
    );
    assert!(
        ejected.sessions_match_terminals(),
        "item 6, on the ejection"
    );
    assert!(target.sessions_match_terminals(), "item 6, on the target");
    let mut words = tab_texts(&target);
    words.sort();
    assert_eq!(
        words,
        vec!["SRCA", "SRCB", "TGTA"],
        "the target kept its other pane, took both arrivals, and gave up exactly one"
    );
}

/// **N160's two halves pull on the same field, and the order they are applied
/// in is the whole of this test.**
///
/// The case that separates them: a **pinned source** merged into an
/// **unpinned target**, at a centre. N160① makes the target pinned — it now
/// holds the thing you asked to have back. N160② gives the ejected pane the
/// pin of the tab it was *living under*, and the tab it was living under was
/// unpinned a moment ago. Read the host's pin after the merge instead of
/// before and the ejected pane comes back pinned on the strength of somebody
/// else's promise, which is a tab reappearing at every launch that nobody
/// ever asked for.
///
/// Its mirror is the test above: an unpinned source into a pinned target
/// ejects a *pinned* pane. Between them the ejected tab's pin is shown to
/// follow the host's own state at the moment of displacement and neither the
/// arriving tab's nor a constant.
///
/// Red gate: move the `host_pinned` read below `absorb_tab_sessions` in
/// [`absorb_tab_into_layout`] and this goes red while its mirror stays green
/// — which is exactly how the bug would have shipped.
#[test]
fn a_pinned_tab_merging_in_does_not_pin_the_pane_it_displaced() {
    let mut source = cross_tab(1, &["SRCA", "SRCB"]);
    source.pinned = true;
    let mut target = cross_tab(2, &["TGTA", "TGTB"]);
    assert!(!target.pinned);
    let displaced = target
        .seats
        .tree()
        .find_seat(SeatId(2))
        .cloned()
        .expect("in the live tree");
    let arrived = cross_merge(
        &source.seats,
        &mut target,
        seats::LayoutAim::SeatCentre(SeatId(2)),
    );
    let ejected = absorb_tab_into_layout(
        &mut source,
        &mut target,
        &arrived,
        Some(&displaced),
        TabId(9),
        cross_solve,
    )
    .expect("the displaced pane had a session to carry");

    assert!(
        target.pinned,
        "N160(1): the target took on the pinned tab's promise"
    );
    assert!(
        !ejected.pinned,
        "N160(2): the pane was living under an unpinned tab when it was displaced"
    );
    assert_eq!(leaf_says(&ejected, SeatId(1)), "TGTB");
}

// ── the plural content plane (slice 5) ──────────────────────────────────

/// PIN (P95 / ruling 8⑧, slice 5) — **two preview surfaces are two views over
/// one pool**, and that is the whole of what the pin was waiting for.
///
/// Slice 4 could pin a pane and could say which pane a new file would land on
/// ([`seats::Seats::landing_preview`]), and then had to open the file over the
/// pinned pane anyway, because the buffer on screen, its document, its caret
/// and its scroll were one set of fields on the tab. This is those fields made
/// plural, asked as a property: pin a pane, open a second file, and the first
/// pane still holds its own buffer at its own scroll.
///
/// The pool is deliberately not part of the split — a file open in two panes
/// is one buffer (§7.1.3, the 2026-07-17 ruling) — so what two surfaces are
/// allowed to disagree about is exactly the width of [`PreviewPane`].
///
/// MUTATIONS:
/// ① make `PreviewPanes::entry` always hand back the first pane (`&mut
///    self.panes[0].1`) — the singleton restored — and the "unchanged" and
///    "own scroll" assertions go red together;
/// ② have `open` below land on `seats.preview()` instead of
///    `seats.landing_preview()` — the second surface is the first, and the
///    same two assertions go red.
#[test]
fn a_pinned_preview_keeps_its_own_buffer_and_scroll_when_the_next_file_opens() {
    let metrics = seats::seat_metrics(1_000);
    let mut seats = seats::Seats::lone_terminal();
    let mut panes = PreviewPanes::default();

    // `open_preview_file`'s landing rule, as a two-line stand-in: the seat
    // layer says where, and the content plane writes there.
    let open = |seats: &mut seats::Seats, panes: &mut PreviewPanes, path: &str| {
        let seat = seats.add_preview(&metrics).expect("a preview lands");
        let surface = seat_of(TAB_ONE, seat);
        panes.entry(surface).buffer = Some(preview::PreviewSource::file(path));
        surface
    };

    let first = open(&mut seats, &mut panes, "a.md");
    panes.entry(first).scroll = [0.0, 120.0];
    assert_eq!(
        open(&mut seats, &mut panes, "b.md"),
        first,
        "un-pinned, the next file reuses the pane — the singleton still holds"
    );
    assert_eq!(
        panes.get(first).expect("a view").buffer.as_ref(),
        Some(&preview::PreviewSource::file("b.md")),
        "and replaces what it was showing"
    );

    // Now pin it, and open a third file.
    let PreviewSurface::Seat(pinned) = first else {
        unreachable!("the landing surface is a seat");
    };
    assert!(seats.toggle_preview_lock(pinned.seat));
    let second = open(&mut seats, &mut panes, "c.rs");
    assert_ne!(second, first, "a locked pane is not the reuse target");

    assert_eq!(
        panes.get(first).expect("a view").buffer.as_ref(),
        Some(&preview::PreviewSource::file("b.md")),
        "the pinned pane is untouched — that is what the pin buys"
    );
    assert_eq!(
        panes.get(first).expect("a view").scroll,
        [0.0, 120.0],
        "including how far down it was"
    );

    // And the two scroll apart without touching one another.
    panes.entry(second).scroll = [0.0, 40.0];
    assert_eq!(panes.get(first).expect("a view").scroll, [0.0, 120.0]);
    assert_eq!(panes.get(second).expect("a view").scroll, [0.0, 40.0]);
    assert_eq!(
        panes.showing(),
        vec![
            preview::PreviewSource::file("b.md"),
            preview::PreviewSource::file("c.rs")
        ],
        "both are on screen, in the order they were first shown"
    );

    // Closing one retires its view and leaves the other's alone.
    assert!(panes.remove(second).is_some());
    assert!(panes.get(second).is_none());
    assert_eq!(
        panes.get(first).expect("a view").buffer.as_ref(),
        Some(&preview::PreviewSource::file("b.md"))
    );
}

/// PIN — **a floating tree that has not been read yet opens at its kind's
/// full height, not as a title strip.**
///
/// The report: every hover peek and every folder button came up as a bare
/// bar hard against a corner. The cause is one line — `place_float` sized
/// the window from `tree_view(...).rows.len()`, and every float is summoned
/// with an empty [`files::DirCache`], so that count is *one* on the opening
/// frame: the `Loading` notice standing in for however many names the worker
/// is about to send.
///
/// Both halves are pinned here, because the fix is worthless if it only ever
/// opens tall: an unread tree takes the cap, and a tree whose rows are in
/// hand takes its rows.
///
/// Mutation: delete the `settled()` guard in `files_float_content_height` so
/// the pending view is measured by its row count — the unread window comes
/// back one row tall, which is the strip, and this fails on the first
/// assertion.
#[test]
fn a_tree_that_is_still_loading_opens_at_the_kinds_height_and_shrinks_when_it_lands() {
    let viewport = [0.0, 0.0, 1200.0, 900.0];
    let scale = 1.0;
    let state = seats::FilesLeafState {
        root: "C:\\work".to_owned(),
        ..seats::FilesLeafState::default()
    };
    let cap = float::float_height_cap(viewport, scale, float::FloatSizing::files());

    // Nobody has answered yet: the one row on screen is a placeholder for an
    // unknown number of them, so the window opens as tall as it is allowed.
    let pending =
        files_float_content_height(&state, &files::DirCache::default(), false, viewport, scale);
    assert!(
        pending >= cap,
        "an unread tree must not be measured by its Loading row: {pending} < {cap}"
    );
    let opened = float::float_opening_size(pending, viewport, scale, float::FloatSizing::files());
    assert!(
        (opened[1] - cap).abs() < 1.0,
        "the opening frame is the kind's own maximum, not a strip: {opened:?}"
    );

    // The rows land, and the same door answers with their height instead —
    // `self_sizing` then walks the window down to it.
    let mut cache = files::DirCache::default();
    cache.accept("", listing(&[("a.rs", false), ("b.rs", false)]));
    let settled = files_float_content_height(&state, &cache, false, viewport, scale);
    // **And a window standing on its Git page opens at the cap whatever its
    // tree has answered** (user ruling, 2026-08-19): the page it is showing
    // is not the list this height is measured from.
    assert_eq!(
        files_float_content_height(&state, &cache, true, viewport, scale),
        pending,
        "a Git page is sized by the cap, not by the tree behind it"
    );
    assert!(
        settled < pending,
        "a tree that has been read is measured by its rows: {settled} !< {pending}"
    );
    assert_eq!(
        settled,
        float::float_height_for_body(seats::files_tree_content_height(2, scale), scale),
        "and by nothing else — two rows are two rows"
    );
}

// ── R31: what it costs to have a Git page you are not looking at ────────

/// PIN (R31 / the master switch's third promise) — **with the panel off, not
/// one process is started.**
///
/// The whole reason the switch exists. A control that only hid the drawing
/// would leave the reading in place, which is the half a user turning this
/// off is actually asking about — and "no `git` runs" is a promise that can
/// only be kept at the one place a question is born.
///
/// The second condition is just as load-bearing: a column merely *having* a
/// Git page available costs nothing either. A repository is read when
/// somebody is looking at it, and at no other time — no timer, no watcher, no
/// "the folder happened to be a repository".
#[test]
fn a_repository_is_read_only_for_a_column_that_is_showing_it() {
    let column = SeatId(1);
    let other = SeatId(2);
    let on_screen = vec![
        (column, seats::FilesView::Git, r"D:\repo".to_owned()),
        (other, seats::FilesView::Files, r"D:\elsewhere".to_owned()),
    ];

    assert_eq!(
        columns_wanting_git(false, &on_screen),
        Vec::new(),
        "the panel is off: nothing is asked, however many Git pages were open"
    );
    assert_eq!(
        columns_wanting_git(true, &on_screen),
        vec![(column, r"D:\repo".to_owned())],
        "and with it on, only the column actually showing the page asks — a \
             column on its tree costs exactly what it cost before this slice"
    );

    // A column with nowhere to stand asks nothing either: an unrooted column
    // has no folder to probe, and `rev-parse` in the empty string is a
    // process spent to be told so.
    assert_eq!(
        columns_wanting_git(true, &[(column, seats::FilesView::Git, "   ".to_owned())]),
        Vec::new()
    );
}

// ── #49: reading-level syntax highlighting ──────────────────────────────

/// A rust file's own [`PreviewDocument`], built the way
/// [`Runtime::rebuild_preview_document`] builds it, without a window.
fn highlighted_text_document(name: &str, body: &str, columns: Option<usize>) -> PreviewDocument {
    let lines = preview_edit::display_lines(body);
    let wrap = match columns {
        Some(columns) => preview_edit::WrapLayout::wrapped(&lines, columns),
        None => preview_edit::WrapLayout::unwrapped(&lines),
    };
    let highlight = highlight::syntax_for_file(name, lines.first().map(String::as_str))
        .map(|grammar| highlight::Highlighting::of(&lines, grammar))
        .unwrap_or_default();
    PreviewDocument::Text {
        lines,
        wrap,
        highlight,
    }
}

/// **A fold does not lose a colour, and does not lose a character** (#49).
///
/// The wrap machinery cuts a line into rows by *column*, and every row used
/// to be exactly one run. Now a row is however many runs its own spans need,
/// and the property that has to survive is the one the caret and the
/// selection depend on: the rows of a line, concatenated, are the line.
///
/// MUTATION: clamp `Highlighting::runs`' span walk to the line's start
/// instead of to `from`, and the second row repeats the first row's text.
#[test]
fn a_wrapped_highlighted_line_keeps_every_character_and_its_inks() {
    let palette = bt_render::chrome_palette();
    let source = "let message = \"a fairly long string literal\"; // and a note\n";
    let document = highlighted_text_document("main.rs", source, Some(20));
    let PreviewDocument::Text {
        lines,
        wrap,
        highlight,
    } = &document
    else {
        panic!("a .rs file is a text document");
    };
    assert!(wrap.rows() > 2, "the fixture really does fold");
    let metrics = seats::preview_text_metrics(1.0);
    let geometry = seats::preview_mono_geometry(
        [0.0, 0.0, 400.0, 400.0],
        metrics,
        metrics.line_height * wrap.rows() as f32,
        20,
        8.0,
        [0.0, 0.0],
    );
    let built = build_preview_text_body(&geometry, lines, wrap, highlight, 8.0, None, &palette);
    assert_eq!(built.paragraphs.len(), wrap.rows());
    let refolded: String = built
        .paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.runs.iter())
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(
        refolded.trim_end(),
        lines[0].trim_end(),
        "the rows of a folded line are that line"
    );
    let inks: Vec<[u8; 3]> = built
        .paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.runs.iter())
        .map(|run| run.color)
        .collect();
    assert!(
        inks.contains(&palette.hl_keyword),
        "`let` survives the fold"
    );
    assert!(inks.contains(&palette.hl_string), "the literal does too");
    assert!(inks.contains(&palette.hl_comment), "and so does the note");
    assert!(
        inks.contains(&palette.preview_body_text),
        "and `message` is still the body's own ink"
    );
    assert!(
        built
            .paragraphs
            .iter()
            .all(|paragraph| paragraph.runs.iter().all(|run| run.mono && !run.bold)),
        "every run is still the monospace face the surface is set in"
    );
}

/// **One document, one highlighting, whichever surface is looking** (#49,
/// ticket clause 7).
///
/// A pane, a torn-off float and the glance card are three rectangles over
/// one [`PreviewDocument`]: the spans are on the document, and
/// `build_preview_text_body` is the only place a text body's runs are made.
/// So the card gets highlighting *because* it shares the document, not
/// because a second path was taught to do the same thing — and this is what
/// says so, by building the same document into two different rectangles and
/// getting the same inks out.
///
/// MUTATION: give either caller its own `Highlighting::default()` instead of
/// the document's and the two ink lists stop matching.
#[test]
fn every_surface_over_one_document_gets_that_documents_own_highlighting() {
    let palette = bt_render::chrome_palette();
    let document = highlighted_text_document("main.rs", "fn main() { let n = 1; }\n", None);
    let PreviewDocument::Text {
        lines,
        wrap,
        highlight,
    } = &document
    else {
        panic!("a .rs file is a text document");
    };
    assert!(!highlight.is_plain(), "the fixture is highlighted at all");
    let metrics = seats::preview_text_metrics(1.0);
    let inks = |body: [f32; 4]| {
        let geometry = seats::preview_mono_geometry(
            body,
            metrics,
            metrics.line_height * wrap.rows() as f32,
            40,
            8.0,
            [0.0, 0.0],
        );
        build_preview_text_body(&geometry, lines, wrap, highlight, 8.0, None, &palette)
            .paragraphs
            .iter()
            .flat_map(|paragraph| paragraph.runs.iter())
            .map(|run| (run.text.clone(), run.color))
            .collect::<Vec<_>>()
    };
    // A docked pane, and the glance card's own smaller box.
    let pane = inks([0.0, 0.0, 800.0, 400.0]);
    let card = inks([120.0, 60.0, 420.0, 200.0]);
    assert_eq!(pane, card, "the card reads the pane's own spans");
    assert!(pane.iter().any(|(_, ink)| *ink == palette.hl_keyword));
}

/// **The cap is a document-level decision, and an over-cap file is plain**
/// (#49).
///
/// Stated at the level a pane sees rather than inside the module, because
/// this is where it would go wrong: the fallback has to produce a document a
/// pane can still draw, scroll and put a caret in — not a document with no
/// paragraphs.
#[test]
fn a_file_over_the_highlight_cap_still_draws_as_a_plain_body() {
    let palette = bt_render::chrome_palette();
    let mut source = String::new();
    for _ in 0..highlight::HIGHLIGHT_MAX_LINES + 1 {
        source.push_str("let x = 1;\n");
    }
    let document = highlighted_text_document("big.rs", &source, None);
    let PreviewDocument::Text {
        lines,
        wrap,
        highlight,
    } = &document
    else {
        panic!("a .rs file is a text document");
    };
    assert!(highlight.is_plain(), "over the cap, nothing is walked");
    let metrics = seats::preview_text_metrics(1.0);
    let geometry = seats::preview_mono_geometry(
        [0.0, 0.0, 400.0, 200.0],
        metrics,
        metrics.line_height * wrap.rows() as f32,
        20,
        8.0,
        [0.0, 0.0],
    );
    let built = build_preview_text_body(&geometry, lines, wrap, highlight, 8.0, None, &palette);
    assert!(!built.paragraphs.is_empty(), "the body is still drawn");
    assert!(
        built
            .paragraphs
            .iter()
            .all(|paragraph| paragraph.runs.len() == 1
                && paragraph.runs[0].color == palette.preview_body_text),
        "and drawn exactly as it was before highlighting existed"
    );
}
