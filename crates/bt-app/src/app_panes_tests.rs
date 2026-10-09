//! **The crate root: panes and layout.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    ResizeGateHarness, calls_of, grid_of, item_body, item_declaration, leaf_saying, method_body,
    on_the_window_thread, pane_box_of, pane_rects_of, reader_names, solved_lopsided_split,
    split_window, squeezed, squeezed_body,
};
use bt_source::ItemQuery;
use std::time::Duration;

/// **The declaration of one field of `owner`** — its attributes, its
/// visibility, its name and its type, stopping before the comma.
fn field_declaration(owner: &str, name: &str) -> &'static str {
    item_declaration(&ItemQuery::field(owner, name))
}

/// PIN (§7.1.6c-3c) — **the layout key this window hands its sessions
/// carries the language's revision as well as the palette's.**
///
/// Red before this slice: `LayoutKey` had four fields and none of them was
/// the language, which is exactly what `crates/bt-app/src/i18n.rs`'s header
/// named as the reason the language could not be switched live.
///
/// It is pinned on this function rather than at either call site because
/// this function *is* the pin: the leaf builder and
/// `Runtime::sync_math_layout_key` both go through it, so a revision left
/// out here is left out of both, and a fifth field added without a line here
/// is a field neither of them fills in.
///
/// MUTATION: hard-code `lang_rev: 0` and this fails as soon as any test in
/// this process has moved the language; hard-code either revision to a
/// constant and it fails immediately.
#[test]
fn the_windows_layout_key_reads_both_of_the_processs_revisions() {
    let key = super::window_layout_key(
        NonZeroU32::new(80).unwrap(),
        NonZeroU32::new(1000).unwrap(),
        NonZeroI64::new(20 * 1024).unwrap(),
        7,
        true,
    );
    assert_eq!(key.width_cells.get(), 80);
    assert_eq!(key.dpi_milli.get(), 1000);
    assert_eq!(key.font_rev, 7);
    assert_eq!(key.font_size_subpixels, 20 * 1024);
    assert_eq!(key.theme_rev, bt_render::theme_revision());
    assert_eq!(key.lang_rev, i18n::lang_revision());
}

/// PIN — **a saved pane comes back as its own shell**, per leaf, and two
/// panes of one tab may be two different ones.
///
/// Red gate, and it is the whole of R-e. `revive_plan` used to read the
/// profile off the *first* term leaf and put it on the tab, so a tab saved
/// as `[pwsh | cmd]` — the very shape `docs/UI-UX.md` §425 names as the
/// reason for per-leaf profiles — came back as two PowerShells. Nothing on
/// disk had to change for that bug: the file said `cmd` in the right place
/// the whole time, and the reader threw it away.
#[test]
fn each_saved_pane_comes_back_as_the_shell_it_was_saved_as() {
    let leaf = |profile_id: &str, cwd: &str| {
        Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
            profile_id: profile_id.to_owned(),
            cwd: cwd.to_owned(),
            manual_name: None,
            card_skip: 0,
            last_command: String::new(),
        })))
    };
    // Two different shells this build ships.
    let (first, second) = match bt_platform::host_platform() {
        bt_platform::HostPlatform::Windows => ("pwsh", "cmd"),
        bt_platform::HostPlatform::MacOs | bt_platform::HostPlatform::OtherUnix => ("bash", "sh"),
    };
    let here = std::env::current_dir().expect("a test runs somewhere");
    let (seats, _, leaves, _files, _preview) = revive_plan(&TabV1 {
        root: LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 500_000,
            children: [leaf(first, &here.to_string_lossy()), leaf(second, "")],
        }),
        pinned: false,
        focused_leaf: "leaf-0".to_owned(),
        preview: None,
    });
    let [left, right] = seats.terminals()[..] else {
        panic!("a row of two terminals holds two terminal seats");
    };
    assert_eq!(
        leaves[&left],
        LeafSeed {
            profile: first.to_owned(),
            cwd: Some(profiles::SeedPlace::Carried(here)),
            unknown_profile_id: None,
            card_skip: 0,
            prefill: None,
            carried_environment: None,
        }
    );
    assert_eq!(
        leaves[&right],
        LeafSeed {
            profile: second.to_owned(),
            cwd: None,
            unknown_profile_id: None,
            card_skip: 0,
            prefill: None,
            carried_environment: None,
        }
    );
    assert_ne!(
        leaves[&left].profile, leaves[&right].profile,
        "two panes, two shells — the tab has no single answer to give"
    );
}

/// PIN (user ruling, 2026-08-15): **the pane head's `⊞` cuts the pane along
/// its longer side.**
///
/// The button makes exactly one promise beyond "a split happens", and it is
/// this one: the halves come out as square as the pane allows, so a tall
/// pane is cut across and a wide one down the middle. It is the promise the
/// glyph is drawn to keep — the 田 is axis-agnostic on purpose, because a
/// glyph showing one vertical rule over a button that sometimes cuts
/// horizontally would be a lie told sixteen pixels wide.
///
/// It is also the one thing about this button that a wrong answer hides. A
/// split always *happens*, so the failure is not an error — it is two long
/// thin panes where two square ones were asked for, and a user shrugging.
///
/// MUTATION: return [`Axis::Row`] unconditionally — "always side by side",
/// the shape this would take if the measurement were dropped — and every
/// tall case goes red. Flip the comparison to `>` and the square case goes
/// red, which is the tie the dev chord's own default settles.
#[test]
fn the_pane_heads_divider_cuts_the_longer_side() {
    assert_eq!(
        auto_split_axis(1600, 900),
        Axis::Row,
        "a wide pane is cut side by side: the new shell goes on the right"
    );
    assert_eq!(
        auto_split_axis(600, 1400),
        Axis::Col,
        "a tall pane is cut across: the new shell goes below"
    );
    assert_eq!(
        auto_split_axis(800, 800),
        Axis::Row,
        "a square pane ties to side by side, where the duplicate chord \
             always went"
    );
    // One pixel either side of the tie, so the boundary is pinned and not
    // merely the two comfortable cases around it.
    assert_eq!(auto_split_axis(801, 800), Axis::Row);
    assert_eq!(auto_split_axis(799, 800), Axis::Col);
}

/// RED (review row R1-24) — **only a line the reader was at is written into
/// the session, and only a bounded one is offered back.**
///
/// The whole of the row lands in three places and this pins the one in this
/// file. `bt_term` records whether the reader's own keyboard reached the pane
/// while a mark was open (`CommandMark::typed_by_user`, tested there);
/// `bt_persist` clears a remembered line no keyboard could have produced
/// (`MAX_LAST_COMMAND_CHARS`, tested there); and this is the join — the
/// document written from a live window keeps a line only from a mark that
/// carries the witness.
///
/// Read off the source because the alternative is a live window with a
/// ConPTY in it: the expression sits inside `TabState::persisted_term_leaf`,
/// which needs a solved layout, a spawned shell and a reader at a keyboard to
/// reach at all. What can go wrong is one clause in one filter, and one
/// clause in one filter is exactly what a source pin can hold.
///
/// Red gate: drop `&& mark.typed_by_user` and this goes red — which is the
/// state in which a program printing `OSC 133` marks decides what the next
/// launch types onto the reader's prompt.
#[test]
fn a_remembered_line_comes_only_from_a_mark_the_reader_was_at() {
    // The deleted reading took eight hundred bytes from the anchor, which ran
    // two hundred past the end of the method it meant; the item stops where
    // the item stops.
    let leaf = method_body("TabState", "term_leaf");
    let at = leaf
        .find("last_command: if remember_the_command {")
        .expect("the one place a remembered line is written");
    let clause = &leaf[at..];
    assert!(
        clause.contains("!mark.command_text.is_empty() && mark.typed_by_user"),
        "the line written into session.json comes from a mark whose command              the reader's own keyboard was present for: {clause}"
    );
}

/// **The takeover begins with the first character, not with the capsule**
/// (§7.1.5d, S4).
///
/// One search in the window, so the pane it is on is the only pane whose rail
/// changes — and an empty field leaves even that one alone. The fourth case is
/// the one the prototype gets wrong and the reason this predicate has a name.
///
/// MUTATION: drop the `is_empty` clause and `Ctrl+F` recedes the command
/// history to a fifth of its opacity before a question has been asked; drop the
/// seat comparison and every pane in the window answers one pane's query.
#[test]
fn a_rail_carries_results_only_on_the_searched_pane_and_only_once_something_is_typed() {
    let searched = SeatId(3);
    let other = SeatId(4);
    assert!(rail_is_searching(Some(searched), "cargo", searched));
    assert!(
        !rail_is_searching(Some(searched), "", searched),
        "an open capsule with nothing typed has asked nothing"
    );
    assert!(
        !rail_is_searching(Some(searched), "cargo", other),
        "one search, one pane"
    );
    assert!(
        !rail_is_searching(None, "cargo", searched),
        "the query outlives the capsule (B62), the takeover does not"
    );
}

/// The resting dots belong to every pane; only the upgrade belongs to the pointer.
///
/// `apply_hover_marks` is the hover half, and both build points run it for `hover_pane` alone
/// — `publish_frame_inner` for the focused leaf, `redraw` for the rest. The resting dotted
/// underline is projection's, applied inside `viewport_frame` for whichever leaf is being
/// built, which is what lets a pane the pointer has never entered still advertise its links.
///
/// Both branches are walked here the way `redraw` walks them: project, then offer the hover
/// marks only to the pane under the pointer. Move the resting mark into `apply_hover_marks`,
/// or gate projection on the hovered leaf, and the quiet pane's link comes out bare.
#[test]
fn a_pane_the_pointer_never_entered_still_wears_the_resting_link_dots() {
    let link = b"\x1b]8;;https://actual.example/login\x1b\\docs\x1b]8;;\x1b\\";
    let pane = || {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(24).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(link).unwrap();
        let projection = session.new_projection(session.layout_key());
        (session, projection)
    };
    let dots = |frame: &ViewportFrame| {
        frame.cells[..4]
            .iter()
            .filter(|cell| {
                cell.style
                    .flags
                    .contains(bt_transcript::CellFlags::DOTTED_UNDERLINE)
            })
            .count()
    };
    let solid = |frame: &ViewportFrame| {
        frame.cells[..4]
            .iter()
            .filter(|cell| {
                cell.style
                    .flags
                    .contains(bt_transcript::CellFlags::UNDERLINE)
            })
            .count()
    };

    // The pane nobody has pointed at: projected, and then left alone.
    let (quiet_session, mut quiet_projection) = pane();
    quiet_session.refresh_projection(&mut quiet_projection);
    let quiet = quiet_session.viewport_frame(&mut quiet_projection).unwrap();
    assert_eq!(
        dots(&quiet),
        4,
        "a link in a pane the pointer has never entered still wears its resting dots"
    );
    assert_eq!(
        solid(&quiet),
        0,
        "and wears nothing the pointer is supposed to have brought"
    );

    // The pane under the pointer: projected, and then decorated.
    let (hovered_session, mut hovered_projection) = pane();
    hovered_session.refresh_projection(&mut hovered_projection);
    let mut hovered = hovered_session
        .viewport_frame(&mut hovered_projection)
        .unwrap();
    let hit = hovered
        .hyperlink_at(0, 1)
        .expect("the link is under the pointer");
    assert!(hovered.underline_hyperlink(&hit));
    assert_eq!(
        solid(&hovered),
        4,
        "the pane under the pointer upgrades the same link to a solid underline"
    );
    assert_eq!(
        dots(&hovered),
        0,
        "and the upgrade replaces the dots rather than doubling them"
    );
}

/// RED ⑤ — **a page follows the pane it is drawn in** (§7.10 ④‴).
///
/// The other way the reported blank is reached, and the one that does not
/// need a single frame to go missing. [`Runtime::move_seat_content`] and
/// [`Runtime::pane_into_new_tab`] carry the seven tables a pane can be
/// holding, and a page is not one of them — it cannot be, because
/// `WindowRuntime::web` belongs to the **window** and those two are functions
/// over a `TabState`. Both halves of a page's key change on a move (the
/// arriving tree re-mints its seat numbers from one, and the tab is a
/// different tab), so a page left behind goes on answering to a name whose
/// tab no longer has that seat — and `advance_web_page` asks exactly that
/// question, calls the page orphaned, and **shuts the browser down**. On the
/// machine: a preview pane dragged onto the tab strip whose `.html` or
/// `.pdf` goes blank while its head and address bar, which travel with the
/// pane, go on naming the file.
///
/// The last block is the trade, and it is why the removes are collected
/// before any insert: a centre landing *displaces* the pane that was there,
/// so the arriving page is filed under the id the departing one is filed
/// under, and an insert interleaved with the removes would have one live
/// browser overwrite the other — lost without going through the door that
/// waits for its process to exit.
///
/// RED GATE: drop the `was != now` clause and a pane that did not travel is
/// taken out of the table and put back, which the trade case turns into a
/// lost controller. Take the call out of any one of the three doors and the
/// pin naming that door goes red — each of those three is the reported blank
/// by a different gesture.
#[test]
fn a_page_follows_the_pane_it_is_drawn_in() {
    let at = |tab: u64, seat: u64| LeafId {
        tab: TabId(tab),
        seat: SeatId(seat),
    };
    let hosted: BTreeSet<LeafId> = [at(1, 2), at(4, 1)].into_iter().collect();

    assert_eq!(
        pages_that_move_with_their_panes(&hosted, &[(at(1, 2), at(9, 1))]),
        vec![(at(1, 2), at(9, 1))],
        "a pane holding a page takes it to its new address"
    );
    assert!(
        pages_that_move_with_their_panes(&hosted, &[(at(1, 7), at(9, 1))]).is_empty(),
        "a pane with no page on it has nothing to carry"
    );
    assert!(
        pages_that_move_with_their_panes(&hosted, &[(at(1, 2), at(1, 2))]).is_empty(),
        "and a pair whose two halves are the same is not a move at all — it \
             must not be taken out of the table and put back, because the next \
             pair in the transaction may be filing something under that key"
    );
    assert_eq!(
        pages_that_move_with_their_panes(&hosted, &[(at(1, 2), at(4, 1)), (at(4, 1), at(1, 2))]),
        vec![(at(1, 2), at(4, 1)), (at(4, 1), at(1, 2))],
        "a trade is two journeys in one transaction, in the order the \
             gesture produced them"
    );

    // The three window-level doors, because a page is the window's and the
    // two tab-level carriers cannot reach it.
    for (door, name) in [
        (
            "a pane torn out into a tab of its own",
            "extract_pane_into_new_tab",
        ),
        ("a pane dropped on another tab", "move_pane_across_tabs"),
        ("a tab merged into another's layout", "absorb_tab"),
    ] {
        let text = squeezed_body("Runtime", name);
        assert!(
            text.contains("self.carry_the_pages_of_moved_panes("),
            "{door} leaves its page behind, and a page left behind is closed \
                 a frame later:\n{text}"
        );
    }

    // And the transaction: every page out of the table before any of them
    // goes back in.
    let carrier = squeezed_body("Runtime", "carry_the_pages_of_moved_panes");
    let removed = carrier
        .find("self.window.web.remove(was)?")
        .expect("the pages are taken out of the window's table");
    // Read from the remove and not from the start of the method: the first
    // `.collect();` here is the one that reads the window's keys, and it
    // stands *before* the removes begin. What the transaction promises is
    // the collect that closes the pass the removes are made in — so that is
    // the one this looks for, and a rewrite that removes and re-files a page
    // in one loop has no `.collect();` between the two at all.
    let collected = removed
        + carrier[removed..]
            .find(".collect();")
            .expect("and gathered before any of them is put back");
    let inserted = carrier
        .find("self.window.web.insert(now,page);")
        .expect("and then filed under their new names");
    assert!(
        removed < collected && collected < inserted,
        "a trade files the arriving page under the departing page's own id, \
             so an insert interleaved with the removes loses a live browser:\n{carrier}"
    );
}

#[test]
fn wheel_flush_runtime_skips_unchanged_panes_but_keeps_thumb_frames() {
    let repaint = squeezed_body("Runtime", "repaint_pane_change_inner");
    assert!(repaint.contains("self.publish_frame_inner(trigger,wheel_view_moved.is_some())?"));
    assert!(repaint.contains("||seat==self.focused_leaf||wheel_view_moved==Some(false)"));
    assert!(repaint.contains("self.represent_on_screen_frame(trigger)"));
    let scroll = squeezed_body("Runtime", "scroll_view_exact_in");
    assert!(scroll.contains("letbefore=leaf.projection.scroll_offset_subpixels();"));
    assert!(scroll.contains("letmoved=leaf.projection.scroll_offset_subpixels()!=before;"));
    let publish = scroll
        .find("self.repaint_pane_change_inner(seat,Some(moved))?")
        .unwrap();
    let thumb = scroll.find("self.woke_terminal_thumb(seat)?").unwrap();
    assert!(
        publish < thumb,
        "the thumb shares a moved view's queued frame"
    );
    let wake = squeezed_body("Runtime", "woke_terminal_thumb");
    assert!(wake.contains("ifself.refresh_overlay(){self.present_chrome_change()?;"));
}

/// **`Alt` is the aim, and nothing else is.**
///
/// The second half names `Ctrl+Alt` deliberately: the shortcut table's AltGr
/// discipline is about chords that would swallow a character, and a notch is
/// not a character — so a reader whose hand is on AltGr still aims rather
/// than discovering that the gesture has holes in it.
#[test]
fn alt_and_a_notch_aims_the_seat_under_the_pointer() {
    for held in [
        ModifiersState::ALT,
        ModifiersState::ALT.union(ModifiersState::SHIFT),
        ModifiersState::ALT.union(ModifiersState::CONTROL),
    ] {
        assert_eq!(
            column_notch(held),
            ColumnNotch::Aim,
            "{held:?} is a wheel with Alt held, which aims"
        );
    }
}

/// PIN — a pane that got a shell nobody asked for says so, in the pane, in
/// its first line.
///
/// `M2-restart-shell-contract.md` §3/§5#3: the swap to the fallback profile
/// is never silent. Red gate, and the thing it catches is the *channel*
/// rather than the words — the notice used to be handed to
/// `TerminalFrame::status_text`, which is drawn on one frame of the focused
/// pane and then dropped, so a pane that fell back while you were looking at
/// another tab announced it to nobody and a pane you scrolled away from
/// could never be asked again. Written into the session it is transcript:
/// still there an hour later, and copyable like anything else in the pane.
#[test]
fn a_pane_that_fell_back_to_another_shell_says_so_in_its_first_line() {
    // What `bt-pty` hands up, including the shape that used to reach the
    // glass: the vendored launcher `Debug`-quotes the command line it built
    // for `CreateProcessW`, `NUL` terminator and all.
    let fallback = bt_pty::ShellFallback {
        requested: std::ffi::OsString::from("D:\\App\\Tool\\Git\\bin\\bash.exe\0"),
        started: bt_pty::WINDOWS_POWERSHELL,
    };
    let banner = fallback_banner(&fallback, "gitbash");
    let mut session = DualPlaneSession::with_quotas_and_cell_height(
        nonzero_u32(80),
        nonzero_u32(6),
        DEFAULT_STAGING_QUOTA,
        DEFAULT_FROZEN_LINE_QUOTA,
        std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
    );
    session.feed(banner.as_bytes()).unwrap();
    let visible = session.terminal().visible_text();
    assert_eq!(
        visible[0].trim_end(),
        "[Folio] Git Bash failed to start; using Windows PowerShell 5.1 instead.",
        "the line names the terminal and both profiles by the names the \
             picker offers them under, and is not mistakable for the shell's \
             own output"
    );
    // Red gate on the whole point of the rework: the executable path, the
    // operating system's account of the failure, and the `NUL` the command
    // line carried are all diagnosis, and diagnosis belongs in the log.
    for debris in [r"D:\App", "bash.exe", "os error", "CreateProcess", "\0"] {
        assert!(
            !visible[0].contains(debris),
            "{debris:?} is debugging output and must not reach the pane: {:?}",
            visible[0]
        );
    }
    // **The swap inside one profile**, which `BT_SHELL` and a removed `pwsh`
    // install both reach. Naming the profiles here would print "PowerShell
    // failed to start; using PowerShell instead", so the executables are
    // named — by file name, the one thing that differs and the only part a
    // reader needs.
    let inside = bt_pty::ShellFallback {
        requested: std::ffi::OsString::from(r"C:\Program Files\PowerShell\7\pwsh.exe"),
        started: bt_pty::WINDOWS_POWERSHELL,
    };
    let banner = fallback_banner(&inside, profiles::fallback_profile_id());
    let mut one = DualPlaneSession::with_quotas_and_cell_height(
        nonzero_u32(80),
        nonzero_u32(6),
        DEFAULT_STAGING_QUOTA,
        DEFAULT_FROZEN_LINE_QUOTA,
        std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
    );
    one.feed(banner.as_bytes()).unwrap();
    assert_eq!(
        one.terminal().visible_text()[0].trim_end(),
        "[Folio] pwsh.exe failed to start; using powershell.exe instead.",
        "one profile, two shells: the shells are what the line can tell apart"
    );

    // And the cursor is left on the line beneath, so the shell's own first
    // prompt does not print over it.
    assert!(
        visible[1].trim().is_empty(),
        "the banner ends its own line: {:?}",
        visible[1]
    );
}

/// **Red gate (user report, 2026-08-15): a peek summoned from a pane head
/// must not roll the rail out.**
///
/// The vertical icon-rail layout, a pointer parked on the folder button in a
/// terminal pane's own head — far out in the panes, nowhere near the rail —
/// and the flyout it summoned standing open. The flyout is right; the rail
/// coming out from the left edge under a pointer that never went near it is
/// the bug.
///
/// All four cells of the grid are asserted together because three of them
/// pass while the fourth is broken: keeping the pointer clause without
/// narrowing the peek clause is the bug itself, and narrowing it to *nothing*
/// would silently repeal G102 — a rail row's own peek overhangs the rail and
/// still has to hold it open.
///
/// Mutation: restore `peek_origin.is_some()` in [`rail_zone_wants_open`] and
/// the two `Pane` rows go red at once.
#[test]
fn only_a_peek_hanging_off_a_rail_row_holds_the_icon_rail_open() {
    // The rail the zone is aiming at while parked: 46 logical px at 1.5×.
    const SCALE: f64 = 1.5;
    let rail_right = f64::from(bt_render::RAIL_PARK_LOGICAL_PX) * SCALE;
    let rail_top = f64::from(bt_render::WINDOW_TITLE_BAR_LOGICAL_PX) * SCALE;

    // A pane head's folder button in a split tab, out where the panes are.
    let on_pane_head = Some(PhysicalPosition::new(rail_right + 400.0, rail_top + 120.0));
    // The same pointer, inside the parked strip.
    let on_the_rail = Some(PhysicalPosition::new(rail_right - 1.0, rail_top + 120.0));

    let tab_peek = Some(float::FloatTrigger::Tab(TabId(1)));
    let pane_peek = Some(float::FloatTrigger::Pane(LeafId {
        tab: TabId(1),
        seat: SeatId(7),
    }));

    assert!(
        !rail_zone_wants_open(on_pane_head, rail_right, rail_top, None, false),
        "a pointer out in the panes with nothing open leaves the rail parked"
    );
    assert!(
        !rail_zone_wants_open(on_pane_head, rail_right, rail_top, pane_peek, false),
        "and a flyout it summoned from the pane's own head is none of the \
             rail's business — the rail must stay parked (2026-08-15)"
    );
    assert!(
        rail_zone_wants_open(on_the_rail, rail_right, rail_top, None, false),
        "the rail's own rectangle is still the whole of the ordinary trigger"
    );
    assert!(
        rail_zone_wants_open(on_pane_head, rail_right, rail_top, tab_peek, false),
        "G102 survives: a peek hanging off a rail row overhangs the rail, so \
             the pointer on it must not read as having left the rail"
    );

    // The caption run is not the rail, however far left the pointer is —
    // and a peek from a rail row still holds it open up there, because the
    // clause is about the window and not about where the hand is.
    let in_the_caption = Some(PhysicalPosition::new(4.0, rail_top - 1.0));
    assert!(!rail_zone_wants_open(
        in_the_caption,
        rail_right,
        rail_top,
        None,
        false
    ));
    // And a pointer that has left the window entirely.
    assert!(!rail_zone_wants_open(
        None, rail_right, rail_top, pane_peek, false
    ));
    assert!(rail_zone_wants_open(
        None, rail_right, rail_top, tab_peek, false
    ));
}

/// **Red gate (user report, 2026-08-25): the menu the sidebar opened is part
/// of the sidebar.**
///
/// `Sidebar: Icons`, the panel rolled out under the hand, `New tab`'s `˅`
/// pressed — and the moment the pointer reached the profile list the rail
/// retracted out from under it. The zone knew one thing about the pointer,
/// "is it in the rail's box", and the list it had just raised stands
/// *beside* that box ([`profiles::MenuSide::Beside`]), so reaching for it
/// read as leaving.
///
/// All four rows are asserted together because three of them pass while the
/// fourth is broken, exactly as in
/// `only_a_peek_hanging_off_a_rail_row_holds_the_icon_rail_open`:
/// holding the panel out for *any* popup would make every pane-head menu a
/// second, invisible rail trigger, which is the 2026-08-15 report all over
/// again — so the flag is the owner's answer and not "a menu is up".
///
/// Mutation: drop the `popup_from_the_rail` arm of [`rail_zone_wants_open`]
/// and the second row goes red, which is the reported bug.
#[test]
fn a_menu_the_rail_opened_holds_the_icon_rail_open() {
    const SCALE: f64 = 1.5;
    let rail_right = f64::from(bt_render::RAIL_PARK_LOGICAL_PX) * SCALE;
    let rail_top = f64::from(bt_render::WINDOW_TITLE_BAR_LOGICAL_PX) * SCALE;

    // The profile list hangs off the `˅` and opens to the right of the
    // panel, so every pixel of it is outside the box the zone measures.
    let in_the_menu = Some(PhysicalPosition::new(rail_right + 90.0, rail_top + 60.0));
    let on_the_rail = Some(PhysicalPosition::new(rail_right - 1.0, rail_top + 120.0));

    // Whether the rail grew this popup, asked the way the window asks it:
    // of `popup_owner`, under a vertical layout out of focus mode.
    let grown_by_the_rail = |popup: Popup| {
        popup_owner(popup, tab_surface(false, seats::TabLayoutMode::Vertical))
            == PopupOwner::Tabs(TabSurface::Rail)
    };

    assert!(
        !rail_zone_wants_open(in_the_menu, rail_right, rail_top, None, false),
        "with nothing open, a pointer out past the panel's edge is a pointer \
             that has left it"
    );
    assert!(
        rail_zone_wants_open(
            in_the_menu,
            rail_right,
            rail_top,
            None,
            grown_by_the_rail(Popup::Profile)
        ),
        "but the list the `˅` raised is the rail's own extension, so the hand \
             on it has not left the rail (user report 2026-08-25)"
    );
    assert!(
        !rail_zone_wants_open(
            in_the_menu,
            rail_right,
            rail_top,
            None,
            grown_by_the_rail(Popup::Pane)
        ),
        "while a menu the *stage* raised holds nothing open — the owner tells \
             the two apart, not a rectangle (2026-08-15's report, kept)"
    );
    assert!(
        rail_zone_wants_open(on_the_rail, rail_right, rail_top, None, false),
        "and the plain rectangle still answers on its own"
    );

    // The corridor: while the menu is up the panel stays out wherever the
    // hand is — the pixels between panel and list that belong to neither,
    // and the window's outside. That is rule ②, and the grace is the popup's
    // own life rather than a second clock.
    assert!(rail_zone_wants_open(
        None,
        rail_right,
        rail_top,
        None,
        grown_by_the_rail(Popup::Profile)
    ));
}

/// PIN (Q190 through the settings path): choosing Horizontal clears the rail
/// outright — no icon rail on screen and not one pixel of terminal kept
/// clear for it.
///
/// This is the mock-up's own recorded failure (line 5640-5642): the
/// `rail-icons` class "survived into horizontal mode, where the rail is
/// `display: none` but the terminal was still keeping its 46px clear — a
/// strip of dead space with nothing in it". The sidebar mode is *kept*
/// across the switch, exactly as `state.railMode` is, so coming back to
/// Vertical brings the rail up in the mode the user last chose; what makes
/// the strip go away is the combination rule, not forgetting the mode.
#[test]
fn choosing_horizontal_leaves_no_rail_and_no_terminal_inset() {
    use seats::{RailMode, TabLayoutMode};
    for mode in [RailMode::Expanded, RailMode::Icons] {
        let railed = rail_state_for(TabLayoutMode::Vertical, mode);
        assert!(railed.terminal_inset_logical_px() > 0.0);
        let flat = rail_state_for(TabLayoutMode::Horizontal, mode);
        assert_eq!(flat.mode, mode, "the sidebar mode is remembered, not reset");
        assert!(!flat.draws_icon_rail());
        assert_eq!(
            flat.terminal_inset_logical_px(),
            0.0,
            "no 46px strip of dead space"
        );
        assert_eq!(
            rail_state_for(TabLayoutMode::Vertical, flat.mode),
            railed,
            "Vertical brings the rail back in the mode that was chosen"
        );
    }
}

/// Reduced motion does not shorten the rail's transitions — it removes them.
///
/// The mock-up's `prefers-reduced-motion` block writes `transition: none`,
/// and `none` is the terminal value *at once*. The half that matters as much
/// is the second return value: a window under `Motion::Reduced` asks for no
/// animation frames at all, which is what lets the event loop go genuinely
/// idle rather than spinning at 60fps drawing the same pixels.
#[test]
fn reduced_motion_puts_the_rail_at_its_target_and_asks_for_no_frames() {
    let start = Instant::now();
    for (span, delay) in [
        (RAIL_TRANSITION, Duration::ZERO),
        (RAIL_TEXT_FADE, RAIL_TEXT_FADE_OPEN_DELAY),
    ] {
        let mut tween = RevealTween::over(span);
        tween.retarget_after(1.0, start, Motion::Reduced, delay);
        for at in [0, 1, 30, 60, 200] {
            assert_eq!(
                tween.sample(start + Duration::from_millis(at), Motion::Reduced),
                (1.0, false),
                "{at}ms in: `transition: none` is the end state now, and no frame is owed"
            );
        }
        tween.retarget(0.0, start, Motion::Reduced);
        assert_eq!(tween.sample(start, Motion::Reduced), (0.0, false));
    }
}

/// PIN (Bug 4): reduced motion folds the rail instantly, as it always did.
///
/// The animation must not become a delay for someone who asked the OS for no
/// animations. Stated on the fold's own tween rather than inherited from the
/// hover's, because "it reuses `RevealTween`" is the implementation and this
/// is the promise.
#[test]
fn reduced_motion_folds_the_rail_with_no_travel_and_no_frames() {
    let start = Instant::now();
    let mut fold = RevealTween::resting(1.0, RAIL_TRANSITION);
    fold.retarget(0.0, start, Motion::Reduced);
    for at in [0, 1, 30, 90, 180, 400] {
        assert_eq!(
            fold.sample(start + Duration::from_millis(at), Motion::Reduced),
            (0.0, false),
            "{at}ms in: the fold is already done and owes no frame"
        );
    }
    fold.retarget(1.0, start, Motion::Reduced);
    assert_eq!(fold.sample(start, Motion::Reduced), (1.0, false));
}

/// PIN (Bug 4): a rail restored collapsed starts folded, not folding.
///
/// [`RevealTween::over`] would have seeded every fold at `0.0` — right for a
/// collapsed rail by accident and wrong for the ordinary one, which would
/// have opened the window with its panel missing until something moved it.
#[test]
fn the_fold_is_seeded_where_the_rail_actually_stands() {
    let now = Instant::now();
    for collapsed in [false, true] {
        let seeded = RevealTween::resting(f32::from(u8::from(!collapsed)), RAIL_TRANSITION);
        assert_eq!(
            seeded.sample(now, Motion::Full),
            (f32::from(u8::from(!collapsed)), false),
            "a window opening with collapsed={collapsed} is already standing there"
        );
    }
}

/// RED — **`ESC[24;8~` is PowerShell's key, and only a PowerShell pane may be sent it.**
///
/// The chord is what `folio.ps1` binds `InvokePrompt` to. It was gated on the input region
/// alone, on the reading that a shell holding no such binding would drop it — true while
/// PowerShell was the only shell here that emitted `OSC 133` at all, and false since
/// `folio.bash` and `folio.zsh` shipped. GNU readline decodes what it recognises of the
/// sequence and inserts the rest as text, so one window resize put `;8~` on a Git Bash prompt
/// and the next `Enter` answered `syntax error near unexpected token ';'`. Two resizes of a
/// WSL pane read `;8~;8~`.
///
/// Three panes in the one state that used to send bytes: a bash door, a PowerShell door, and a
/// PowerShell door whose prompt has closed. Only the middle one may hear anything.
#[test]
fn the_resize_anchor_chord_goes_to_a_powershell_pane_and_to_no_other_shell() {
    let start = Instant::now();
    let open_prompt = |session: &mut DualPlaneSession| {
        session
            .feed_at(b"\x1b]133;A\x07$ \x1b]133;B\x07", start)
            .unwrap();
    };
    let commit = |session: &mut DualPlaneSession, integration| {
        let mut pending = false;
        commit_leaf_resize(
            session,
            None,
            ResizeReanchor {
                pending: &mut pending,
                integration,
            },
            ReleaseGrids {
                local: grid_of(80, 24),
                conpty: grid_of(80, 24),
                next: grid_of(60, 24),
            },
            PhysicalSize::new(480, 600),
            start,
        )
        .unwrap();
        let at_a_prompt = session.shell_prompt_opened_in_order();
        take_psreadline_resize_reanchor_input(
            ResizeReanchor {
                pending: &mut pending,
                integration,
            },
            at_a_prompt,
        )
    };

    let mut bash = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    open_prompt(&mut bash);
    assert!(
        bash.shell_input_region_open(),
        "the fixture has to leave a prompt open, or there is nothing to withhold"
    );
    assert_eq!(
        commit(&mut bash, profiles::Integration::BashInitFile),
        None,
        "a resize types nothing into a bash prompt: readline would insert what it cannot decode"
    );

    let mut zsh = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    open_prompt(&mut zsh);
    assert_eq!(
        commit(&mut zsh, profiles::Integration::ZshDotDir),
        None,
        "and nothing into a zsh prompt, which is served through its own door"
    );

    let mut cmd = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    open_prompt(&mut cmd);
    assert_eq!(
        commit(&mut cmd, profiles::Integration::CmdPrompt),
        None,
        "nor into a cmd prompt, whose PROMPT integration binds no keys at all"
    );

    let mut bare = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    open_prompt(&mut bare);
    assert_eq!(
        commit(&mut bare, profiles::Integration::None),
        None,
        "nor into a shell this product installed nothing in, whatever it emits"
    );

    let mut powershell = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    open_prompt(&mut powershell);
    assert_eq!(
        commit(&mut powershell, profiles::Integration::PowerShellOptIn),
        Some(PSREADLINE_INVOKE_PROMPT_INPUT),
        "the pane the chord was cut for still gets its anchor repaired"
    );

    let mut closed = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    closed
        .feed_at(b"\x1b]133;A\x07PS> \x1b]133;B\x07ls\r\x1b]133;C\x07", start)
        .unwrap();
    assert!(
        !closed.shell_input_region_open(),
        "the fixture has to leave the region closed, or the door is the only thing tested"
    );
    assert_eq!(
        commit(&mut closed, profiles::Integration::PowerShellOptIn),
        None,
        "a PowerShell running a command is not at a prompt, and hears nothing"
    );
}

/// A debt an unfocused pane records has to be a debt it can pay.
///
/// `finish_resize_if_quiescent` skips any leaf whose transaction has not gone quiet, and a
/// transaction cannot go quiet until a final request has been sent — `quiescence_deadline`
/// answers `None` until then, so `is_quiescent_at` is false at every instant there is. Commit
/// one unfocused leaf's resize exactly as the layout solve does, then run the clock the way
/// the event loop does; the chord has to come out the other side.
///
/// Withhold the reconcile from `commit_leaf_resize` and this goes red at the
/// deadline: that is the shape of the bug, where every pane but the focused one banked a
/// repair it would never be allowed to pay and kept a PSReadLine anchor a size out of date.
#[test]
fn an_unfocused_leaf_resize_reaches_the_quiescence_that_pays_its_reanchor_debt() {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    session
        .feed_at(b"\x1b]133;A\x07PS> \x1b]133;B\x07", start)
        .unwrap();
    assert!(
        session.shell_input_region_open(),
        "the fixture has to leave a prompt open, or there is no debt to test"
    );

    let mut pending = false;
    commit_leaf_resize(
        &mut session,
        None,
        ResizeReanchor {
            pending: &mut pending,
            integration: profiles::Integration::PowerShellOptIn,
        },
        ReleaseGrids {
            local: grid_of(80, 24),
            conpty: grid_of(80, 24),
            next: grid_of(60, 24),
        },
        PhysicalSize::new(480, 600),
        start,
    )
    .unwrap();
    assert!(pending, "a reflowed open prompt owes one repair");

    let deadline = session.resize_finish_deadline().expect(
        "an unfocused leaf's committed resize must arm a quiescence deadline, or the debt it \
             just recorded can never be paid",
    );
    assert!(
        !session
            .finish_resize_if_quiescent(deadline - Duration::from_millis(1))
            .unwrap(),
        "the transaction is still open one millisecond early"
    );
    assert!(
        session.finish_resize_if_quiescent(deadline).unwrap(),
        "the transaction closes at its own deadline"
    );
    assert_eq!(
        take_psreadline_resize_reanchor_input(
            ResizeReanchor {
                pending: &mut pending,
                integration: profiles::Integration::PowerShellOptIn,
            },
            session.shell_input_region_open(),
        ),
        Some(PSREADLINE_INVOKE_PROMPT_INPUT),
        "the pane nobody is watching gets the same anchor repair as the focused one"
    );
}

/// RED — **the release of such a gesture closes the transaction and writes the child
/// nothing** (user report 2026-09-17).
///
/// The other half of the same defect, at the door that actually commits one. No
/// `ResizePseudoConsole` — the child's size never moved, so conhost never reflowed and
/// PSReadLine's anchor is still the one it drew with, which is why no repair may be recorded
/// and not one private byte may be written. The settlement is owed all the same: it is the
/// only thing that closes the transaction `resize_at` opened, and until it does the pane
/// scans for no decoration at all.
///
/// Red gate: send both endings through `mark_pty_resize_requested_at` and the chord comes out
/// of a gesture the shell never heard about; take the queue away and the transaction never
/// closes.
#[test]
fn a_drag_that_ends_where_the_child_already_is_settles_without_telling_it() {
    let start = Instant::now();
    let mut leaf = leaf_saying("one two three four");
    leaf.integration = profiles::Integration::PowerShellOptIn;
    leaf.session
        .feed_at(b"\x1b]133;A\x07PS> \x1b]133;B\x07", start)
        .unwrap();
    assert!(
        leaf.session.shell_input_region_open(),
        "the fixture has to leave a prompt open, or the silence proves nothing"
    );
    let _ = leaf.session.take_pty_writes();
    let physical = PhysicalSize::new(400, 96);
    let step = |leaf: &mut LeafSession, grid, at| {
        schedule_leaf_grid_change(
            leaf,
            grid,
            physical,
            at,
            LeafOnStage::Shown,
            "drag",
            card_trace::Pane::untraced(),
        )
        .unwrap()
    };

    // One ordinary gesture, released and settled: the child now holds 44x4.
    let settled = grid_of(44, 4);
    assert!(step(&mut leaf, settled, start));
    let released_at = start + WINDOW_RESIZE_QUIET;
    let (commit, _) = release_due_leaf_resize(&mut leaf, released_at, false).unwrap();
    let commit = commit.expect("a drag onto a new width owes its child one notification");
    assert!(commit.told_the_child);
    assert_eq!(leaf.conpty_grid, settled);
    let deadline = leaf.session.resize_finish_deadline().unwrap();
    assert!(leaf.session.finish_resize_if_quiescent(deadline).unwrap());
    assert!(
        take_psreadline_resize_reanchor_input(
            ResizeReanchor {
                pending: &mut leaf.pending_psreadline_resize_reanchor,
                integration: leaf.integration,
            },
            leaf.session.shell_input_region_open(),
        )
        .is_some(),
        "that one reflowed the child's screen buffer, so it owes the anchor repair"
    );

    // The wobble: one column away and straight back to the width the child already holds.
    let wobble_at = deadline + Duration::from_millis(100);
    assert!(step(&mut leaf, grid_of(43, 4), wobble_at));
    assert!(step(
        &mut leaf,
        settled,
        wobble_at + Duration::from_millis(17)
    ));
    let released_at = wobble_at + Duration::from_millis(17) + WINDOW_RESIZE_QUIET;
    let (commit, _) = release_due_leaf_resize(&mut leaf, released_at, false).unwrap();
    let commit = commit.expect(
        "the end of a gesture is released whether or not it moved the \
                                    child",
    );
    assert!(
        !commit.told_the_child,
        "the child is already at this size: telling it again is a reflow nobody asked for"
    );
    assert!(
        commit.reconciled,
        "and the transaction is settled all the same"
    );
    assert_eq!(leaf.session.live_dimensions().0.get(), 44);

    let deadline = leaf
        .session
        .resize_finish_deadline()
        .expect("a settled gesture arms a quiescence deadline, whichever ending it had");
    assert!(
        leaf.session.finish_resize_if_quiescent(deadline).unwrap(),
        "and closes at it, which is what lets this pane scan for formulas again"
    );
    assert_eq!(
        take_psreadline_resize_reanchor_input(
            ResizeReanchor {
                pending: &mut leaf.pending_psreadline_resize_reanchor,
                integration: leaf.integration,
            },
            leaf.session.shell_input_region_open(),
        ),
        None,
        "conhost never reflowed, so PSReadLine's anchor is right and owes no repair"
    );
    assert_eq!(
        leaf.session.take_pty_writes(),
        Vec::<Vec<u8>>::new(),
        "not one private byte is written to a shell that was told nothing"
    );
}

/// RED — **a repair the gesture before it banked is paid once, by the settlement that finds
/// it** (Codex review 2026-09-17).
///
/// The sibling of the test above, at the timing it deliberately leaves out: there the earlier
/// commit's anchor repair had already been paid before the wobble began, so nothing was owed
/// across it. Here the hand comes back before the first transaction has gone quiet, which
/// re-opens it — `resize_at` calls `ResizeEpoch::changed` — while the debt that first,
/// *real* `ResizePseudoConsole` banked is still unpaid. Two things have to hold at once: the
/// wobble banks none of its own, because its child was told nothing and conhost never
/// reflowed; and the one that is owed survives to the quiescence that finally arrives, and is
/// paid exactly once.
///
/// Red gate: *clear* the debt on the unchanged ending — `*reanchor.pending = false` — and a
/// real reflow's repair is swallowed, which is the swallowed first character at the prompt.
/// The mutation in the other direction, recording a debt there, is caught by the test above:
/// its earlier repair is already paid, so a new one shows up as a chord sent for a gesture the
/// shell never heard about.
#[test]
fn a_repair_owed_before_a_wobble_is_still_owed_after_it_and_paid_once() {
    let start = Instant::now();
    let mut leaf = leaf_saying("one two three four");
    leaf.integration = profiles::Integration::PowerShellOptIn;
    leaf.session
        .feed_at(b"\x1b]133;A\x07PS> \x1b]133;B\x07", start)
        .unwrap();
    assert!(leaf.session.shell_input_region_open());
    let physical = PhysicalSize::new(400, 96);
    let step = |leaf: &mut LeafSession, grid, at| {
        schedule_leaf_grid_change(
            leaf,
            grid,
            physical,
            at,
            LeafOnStage::Shown,
            "drag",
            card_trace::Pane::untraced(),
        )
        .unwrap()
    };

    // A real gesture onto a new width: the child is told and the repair is banked.
    let settled = grid_of(44, 4);
    assert!(step(&mut leaf, settled, start));
    let at = start + WINDOW_RESIZE_QUIET;
    let commit = release_due_leaf_resize(&mut leaf, at, false)
        .unwrap()
        .0
        .expect("a drag onto a new width is released");
    assert!(commit.told_the_child);
    assert!(
        leaf.pending_psreadline_resize_reanchor,
        "conhost reflowed this shell's screen buffer, so a repair is owed"
    );

    // And the hand comes back before that transaction has gone quiet, ending on the width the
    // child already holds. Nothing is paid in between.
    let wobble_at = at + Duration::from_millis(50);
    assert!(step(&mut leaf, grid_of(43, 4), wobble_at));
    assert!(step(
        &mut leaf,
        settled,
        wobble_at + Duration::from_millis(17)
    ));
    let released_at = wobble_at + Duration::from_millis(17) + WINDOW_RESIZE_QUIET;
    let commit = release_due_leaf_resize(&mut leaf, released_at, false)
        .unwrap()
        .0
        .expect("the end of the wobble is released");
    assert!(!commit.told_the_child, "and tells the child nothing");
    assert!(
        leaf.pending_psreadline_resize_reanchor,
        "so the repair the first gesture banked is still the only one owed"
    );

    let deadline = leaf
        .session
        .resize_finish_deadline()
        .expect("the settlement arms the quiescence that pays it");
    assert!(leaf.session.finish_resize_if_quiescent(deadline).unwrap());
    let reanchor = |leaf: &mut LeafSession| {
        let integration = leaf.integration;
        let open = leaf.session.shell_input_region_open();
        take_psreadline_resize_reanchor_input(
            ResizeReanchor {
                pending: &mut leaf.pending_psreadline_resize_reanchor,
                integration,
            },
            open,
        )
    };
    assert_eq!(
        reanchor(&mut leaf),
        Some(PSREADLINE_INVOKE_PROMPT_INPUT),
        "one reflow of the child's screen buffer, one repair"
    );
    assert_eq!(reanchor(&mut leaf), None, "and it is paid exactly once");
}

/// RED — **one shape, many entrances** (independent review 2026-09-17).
///
/// The leak is never about window edges. It is `schedule_leaf_grid_change` reflowing a shown
/// pane — `next_grid != local_grid`, so `resize_at` opens a transaction — on a solve that
/// answers the grid the child already holds, so the release used to be cancelled and nothing
/// was left that could close it. Every way a pane's grid is solved reaches that one shape:
/// a window edge, a divider let go where it started, a zoom stepped up and back down, a
/// monitor change that nets out, a window minimised mid-drag and restored, and the panes of
/// the active tab that do not hold the keyboard — `resize_leaves_to_layout` sends those
/// through `LeafOnStage::Shown` exactly like the focused one. So the fix is at the shape and
/// this is the sweep that says so.
///
/// The last case is the other side of the same sentence: a pane behind another tab returns
/// before `resize_at`, never reflows and never opens a transaction, so it has nothing to
/// close and is right to owe nothing.
///
/// Red gate: put the `*pending = None` back in `coalesce_pty_resize_on_grid_change`'s `else`.
/// Every entrance but the last then ends with no release and an open transaction.
#[test]
fn every_gesture_that_lands_back_on_the_childs_grid_settles_whatever_moved_it() {
    let child = grid_of(44, 4);
    let child_physical = PhysicalSize::new(352, 96);
    // What the hand was doing, and the one rectangle this pane is solved to on its way there
    // and back. Every entrance differs in what produced the solve, never in the solve.
    let entrances: &[(&str, GridSize, PhysicalSize<u32>)] = &[
        (
            "a window edge dragged out and back",
            grid_of(43, 3),
            PhysicalSize::new(344, 72),
        ),
        (
            "a divider dragged across and back",
            grid_of(40, 4),
            PhysicalSize::new(320, 96),
        ),
        (
            // A font step re-solves the same pixels into a different grid.
            "a zoom stepped up and back down",
            grid_of(36, 3),
            PhysicalSize::new(352, 96),
        ),
        (
            "a window minimised mid-drag and restored",
            grid_of(2, 1),
            PhysicalSize::new(16, 24),
        ),
        (
            "a monitor change that lands back on the same grid",
            grid_of(52, 5),
            PhysicalSize::new(832, 240),
        ),
    ];

    for (gesture, away, away_physical) in entrances {
        for stage in [LeafOnStage::Shown, LeafOnStage::Behind] {
            let start = Instant::now();
            let mut leaf = leaf_saying("one two three four");
            leaf.integration = profiles::Integration::PowerShellOptIn;
            leaf.session
                .feed_at(b"\x1b]133;A\x07PS> \x1b]133;B\x07", start)
                .unwrap();
            let _ = leaf.session.take_pty_writes();

            // The pane and its child both start settled at the same grid.
            schedule_leaf_grid_change(
                &mut leaf,
                child,
                child_physical,
                start,
                LeafOnStage::Shown,
                "settle",
                card_trace::Pane::untraced(),
            )
            .unwrap();
            let at = start + WINDOW_RESIZE_QUIET;
            release_due_leaf_resize(&mut leaf, at, false)
                .unwrap()
                .0
                .expect("the fixture's own first gesture");
            let deadline = leaf.session.resize_finish_deadline().unwrap();
            assert!(leaf.session.finish_resize_if_quiescent(deadline).unwrap());
            let _ = take_psreadline_resize_reanchor_input(
                ResizeReanchor {
                    pending: &mut leaf.pending_psreadline_resize_reanchor,
                    integration: leaf.integration,
                },
                leaf.session.shell_input_region_open(),
            );
            assert_eq!(leaf.conpty_grid, child);

            // Out, and back to the grid the child already holds.
            let mut at = deadline + Duration::from_millis(100);
            for (grid, physical) in [(*away, *away_physical), (child, child_physical)] {
                schedule_leaf_grid_change(
                    &mut leaf,
                    grid,
                    physical,
                    at,
                    stage,
                    "wobble",
                    card_trace::Pane::untraced(),
                )
                .unwrap();
                at += Duration::from_millis(17);
            }

            let released_at = at + WINDOW_RESIZE_QUIET;
            let (commit, _) = release_due_leaf_resize(&mut leaf, released_at, false).unwrap();
            if stage == LeafOnStage::Behind {
                let commit = commit.expect("a release is due either way");
                assert_eq!(
                    commit,
                    LeafResizeCommit::default(),
                    "{gesture}, behind another tab: its actor never followed the solve, so \
                         nothing reflowed, nothing is told and nothing is settled"
                );
                assert_eq!(
                    leaf.conpty_grid, child,
                    "{gesture}, behind another tab: and the release carries the last grid it \
                         was solved to, never the width the hand passed through"
                );
                assert!(
                    !leaf.session.resize_transaction_open(),
                    "{gesture}, behind another tab: no transaction was ever opened"
                );
                continue;
            }
            let commit = commit.unwrap_or_else(|| {
                panic!("{gesture}: the end of a gesture is owed whatever moved it")
            });
            assert!(
                !commit.told_the_child,
                "{gesture}: the child is already at this grid and must not be told again"
            );
            assert!(
                commit.reconciled,
                "{gesture}: and the transaction is settled"
            );
            let deadline = leaf.session.resize_finish_deadline().unwrap_or_else(|| {
                panic!("{gesture}: a settled gesture arms a quiescence deadline")
            });
            assert!(
                leaf.session.finish_resize_if_quiescent(deadline).unwrap(),
                "{gesture}: and closes at it"
            );
            assert_eq!(
                take_psreadline_resize_reanchor_input(
                    ResizeReanchor {
                        pending: &mut leaf.pending_psreadline_resize_reanchor,
                        integration: leaf.integration,
                    },
                    leaf.session.shell_input_region_open(),
                ),
                None,
                "{gesture}: conhost never reflowed, so no anchor repair is owed"
            );
            assert_eq!(
                leaf.session.take_pty_writes(),
                Vec::<Vec<u8>>::new(),
                "{gesture}: and not one byte reaches a shell that was told nothing"
            );
        }
    }
}

/// RED — **a pane without the keyboard owes one ConPTY notification per drag, not one per
/// OS event** (window-thread unbounded-call sweep, 2026-08-24).
///
/// The 200 ms quiet window belonged to the focused leaf alone. Its siblings were told
/// unconditionally from `resize_leaves_to_layout`: one `ResizePseudoConsole` per pane per
/// `Resized`, on the window thread, each one a synchronous re-entry into conhost that
/// reflows the child's screen buffer and banks a PSReadLine anchor repair. A four-pane split
/// under a live drag therefore made three of those calls sixty times a second — the pane
/// holding the keyboard was never the expensive one, only the one whose coalescer was
/// noticed.
///
/// The two halves this pins are the whole ruling (2026-08-06 — 实时放行 resize): **the
/// picture is never debounced** (`leaf.grid` holds the size solved this very turn, so the
/// glass follows the hand) and **the child is** (`conpty_grid` does not move until the
/// gesture has been quiet).
///
/// Red gate: make `schedule_leaf_grid_change` commit instead of coalescing — which is what
/// the unfocused path did — and `conpty_grid` moves inside the loop, sixty times, and the
/// release at the end has nothing left to carry.
#[test]
fn a_pane_without_the_keyboard_coalesces_a_drag_into_one_conpty_notification() {
    let start = Instant::now();
    let mut leaf = leaf_saying("sibling");
    let born = leaf.grid;

    // One second of a live drag, at the rate an OS resize loop delivers.
    const EVENTS: u32 = 60;
    const FRAME: Duration = Duration::from_millis(16);
    let mut last = born;
    for step in 0..EVENTS {
        let at = start + FRAME * step;
        // From the column after the one it was born at, so the very first event of the drag
        // is already a rectangle the pane does not hold.
        let next = grid_of(41 + u16::try_from(step).unwrap(), 4);
        let physical = PhysicalSize::new(u32::from(next.columns.get()) * 8, 88);
        assert!(
            schedule_leaf_grid_change(
                &mut leaf,
                next,
                physical,
                at,
                LeafOnStage::Shown,
                "drag",
                card_trace::Pane::untraced()
            )
            .unwrap(),
            "every one of these rectangles is a new one, so every one is a reflow"
        );
        assert_eq!(
            leaf.grid, next,
            "the picture is not debounced: the actor holds the size solved this turn"
        );
        assert_eq!(
            leaf.conpty_grid, born,
            "and the child has heard nothing yet — it is the notification that waits"
        );
        let (commit, wake) = release_due_leaf_resize(&mut leaf, at, false).unwrap();
        assert!(
            commit.is_none(),
            "mid-drag there is nothing due: every event pushed the quiet boundary out"
        );
        assert_eq!(
            wake,
            Some(at + WINDOW_RESIZE_QUIET),
            "and the loop is asked to come back at that boundary"
        );
        last = next;
    }

    let quiet = start + FRAME * (EVENTS - 1) + WINDOW_RESIZE_QUIET;
    let (commit, wake) = release_due_leaf_resize(&mut leaf, quiet, false).unwrap();
    let commit = commit.expect("the quiet boundary releases the one notification a drag owes");
    assert!(
        !commit.reflowed,
        "our own actor was never behind, so the release defers no reflow to here"
    );
    assert_eq!(
        leaf.conpty_grid, last,
        "the child hears the last word of the gesture and none of the sixty before it"
    );
    assert_eq!(
        wake, None,
        "nothing is owed, so nothing has to be woken for"
    );

    let (again, _) =
        release_due_leaf_resize(&mut leaf, quiet + WINDOW_RESIZE_QUIET, false).unwrap();
    assert!(
        again.is_none(),
        "one drag is one notification; a second turn finds the queue empty"
    );
}

/// RED — **the pane on the glass re-wraps at every width the hand passes through, and a pane
/// behind another tab reflows once for the whole gesture** (user report 2026-09-09).
///
/// Two halves of the 2026-08-06 ruling, and the second is the one that broke. The walk that
/// carries a window's rectangle to every tab arrived on 2026-09-09 with the timing of a pane
/// on the stage: `resize_at` — a full vendor reflow of that pane's grid — for every hidden
/// leaf, on the window thread, inside every OS `Resized`. Measured on the reporter's machine
/// with six tabs open, that put the frame the visible pane owes the glass behind five reflows
/// of panes nobody could see, and the picture stopped following the hand: event-to-present
/// went from 82 ms to 117 ms and the gap between pictures from 128 ms to 186 ms, growing with
/// the number of tabs and flat in it before. The ruling those reflows were paying is a ruling
/// about the *picture*, and a hidden pane has none.
///
/// So the front half here is the ruling said in frames rather than in fields: between two
/// steps of a drag, the pane on the glass **projects** a frame at the width the hand is at.
/// The back half is the fix: `Behind` schedules and does not reflow, its actor is untouched
/// through all sixty events, and the one reflow it owes lands at the quiet boundary with the
/// notification its child was already waiting for.
///
/// Red gate: give the hidden leaf `LeafOnStage::Shown` — which is what the window did — and
/// its actor moves sixty times instead of once, and the release finds nothing left to reflow.
#[test]
fn a_drag_rewraps_the_pane_on_the_glass_and_reflows_a_hidden_pane_once() {
    let start = Instant::now();
    // Long enough that every width in the drag wraps it differently, so a reflow is real work
    // and a frame's column count is a reading of the width it was projected at.
    let printed = "#".repeat(197);
    let mut shown = leaf_saying(&printed);
    let mut behind = leaf_saying(&printed);
    let born = shown.grid;
    assert_eq!(
        behind.grid, born,
        "both fixtures start where the other does"
    );

    const EVENTS: u32 = 60;
    const FRAME: Duration = Duration::from_millis(16);
    let mut last = born;
    for step in 0..EVENTS {
        let at = start + FRAME * step;
        let next = grid_of(41 + u16::try_from(step).unwrap(), 4);
        let physical = PhysicalSize::new(u32::from(next.columns.get()) * 8, 88);

        assert!(
            schedule_leaf_grid_change(
                &mut shown,
                next,
                physical,
                at,
                LeafOnStage::Shown,
                "drag",
                card_trace::Pane::untraced()
            )
            .unwrap(),
            "every one of these rectangles is a new one, so every one is a reflow"
        );
        // The two lines the window runs between the solve and the frame:
        // `sync_math_layout_key` amends the key with the width that moved,
        // and the projection is taken against it.
        let mut key = shown.session.layout_key();
        key.width_cells = nonzero_u32(next.columns.get());
        shown.session.set_layout_key(key);
        shown.projection = shown.session.new_projection(key);
        let frame = shown
            .session
            .viewport_frame(&mut shown.projection)
            .expect("project the pane on the glass between two steps of the drag");
        assert_eq!(
            frame.columns.get(),
            u32::from(next.columns.get()),
            "the frame between two resize steps carries the width the hand is at, not the \
                 width it started from"
        );

        assert!(
            !schedule_leaf_grid_change(
                &mut behind,
                next,
                physical,
                at,
                LeafOnStage::Behind,
                "drag",
                card_trace::Pane::untraced()
            )
            .unwrap(),
            "a pane behind another tab reports no reflow, because it did none"
        );
        assert_eq!(
            behind.session.live_dimensions().0.get(),
            u32::from(born.columns.get()),
            "and its actor is where it was: the window thread spent nothing on a picture \
                 nobody can see"
        );
        assert_eq!(
            behind.grid, born,
            "the shadow follows the actor, so it has not moved either"
        );
        last = next;
    }

    let quiet = start + FRAME * (EVENTS - 1) + WINDOW_RESIZE_QUIET;
    let (commit, _) = release_due_leaf_resize(&mut behind, quiet, false).unwrap();
    let commit = commit.expect("the quiet boundary releases what the hidden pane owes");
    assert!(
        commit.reflowed,
        "and the one reflow a hidden pane owes a gesture lands here"
    );
    assert_eq!(
        behind.session.live_dimensions().0.get(),
        u32::from(last.columns.get()),
        "at the last width of the gesture, not at an intermediate one"
    );
    assert_eq!(behind.grid, last, "with the shadow caught up");
    assert_eq!(
        behind.conpty_grid, last,
        "in the same breath as the notification its child was waiting for"
    );

    let (commit, _) = release_due_leaf_resize(&mut shown, quiet, false).unwrap();
    let commit = commit.expect("the pane on the glass owes its child the same notification");
    assert!(
        !commit.reflowed,
        "its own actor was never behind, so the release defers no reflow to here"
    );
}

/// PIN: moving the keyboard changes which carry is gated, and no leaf's
/// target size.
///
/// The other half of the same rule. `focused` is allowed to decide *when* a
/// shell hears a new size — the 200ms ConPTY quiet window hangs off it — and
/// is allowed to decide nothing about *what* that size is. Solving the
/// identical layout twice, differing only in which
/// leaf holds the keyboard, must produce the identical rectangles; only the
/// flag moves. Let focus back into the geometry and this goes red.
#[test]
fn moving_the_keyboard_changes_which_carry_is_gated_and_no_leaf_target_size() {
    let dpi_milli = 1_000;
    let render_physical = PhysicalSize::new(1600, 900);
    let scale = dpi_milli as f32 / 1_000.0;
    let (seats, layout) = solved_lopsided_split(dpi_milli, render_physical);
    let primary = seats.identity();
    let narrow = seats
        .terminals()
        .into_iter()
        .find(|seat| *seat != primary)
        .expect("the second leaf is not the primary seat");

    let with_primary = leaf_resize_plan(&seats, &layout, primary, scale);
    let with_narrow = leaf_resize_plan(&seats, &layout, narrow, scale);

    let sizes = |plan: &[LeafResizeTarget]| -> Vec<(SeatId, bt_render::SeatViewport)> {
        plan.iter()
            .map(|target| (target.seat, target.body))
            .collect()
    };
    assert_eq!(
        sizes(&with_primary),
        sizes(&with_narrow),
        "a focus change re-sizes nothing; it can only re-send"
    );

    let gated = |plan: &[LeafResizeTarget]| -> Vec<SeatId> {
        plan.iter()
            .filter(|target| target.focused)
            .map(|target| target.seat)
            .collect()
    };
    assert_eq!(gated(&with_primary), vec![primary]);
    assert_eq!(gated(&with_narrow), vec![narrow]);
}

/// **PIN (user report, 2026-09-01, second reading) — a pane that was made narrow and then
/// made wide again leaves its shell at the wide width, on every timing.**
///
/// The report widened the search from "a restored window" to "any narrowing": a divider
/// dragged in and out, a preview opened and closed, a window minimized and restored — all of
/// them squeeze a pane and let it go, and the complaint is that the shell does not come back.
/// The suspicion that goes with it is a *directional* one: that the coalescer keeps the
/// shrink and drops the grow.
///
/// This drives the shipping decision functions — `plan_grid_change`, which is the only thing
/// that ever queues a ConPTY resize, and `service_pending_pty_resize`, which is the only
/// thing that ever releases one — over both timings a gesture can have, and reads back the
/// **whole sequence** the fake ConPTY was handed. There is no third timing: either the quiet
/// window elapsed while the pane was narrow or it did not.
///
/// * **The gesture outlives the quiet window** (a slow drag, a preview left open, a window
///   left minimized). The narrow size is committed on the way down, so the sequence must end
///   with the wide one on the way back up.
/// * **The gesture is over inside the quiet window** (a flick of the divider). Nothing was
///   ever sent, and nothing must be: what the child holds is already right. The gesture still
///   *ends* — this pane's own actor followed the hand down to sixteen columns and back, and
///   the transaction that reflow opened is closed by the release like any other (2026-09-17).
///
/// Red gate: make `coalesce_pty_resize_on_grid_change` refuse to queue a grid wider than
/// `conpty_grid` — the shape the report's own hypothesis describes — and the first case's
/// last element stays at 20 columns while the second case is untouched. Tell the child on the
/// unchanged ending — drop `commit_leaf_resize`'s `told_the_child` split — and the second case
/// grows an entry. Drop the release for that ending instead, and it settles nothing and its
/// transaction never closes.
#[test]
fn a_pane_squeezed_narrow_and_let_go_leaves_its_shell_wide() {
    let start = Instant::now();
    let wide = PhysicalSize::new(2880, 1800);
    // A third of the window, which is what a divider dragged well over is worth.
    let narrow = PhysicalSize::new(960, 1800);

    // ① The squeeze outlasts the quiet window.
    let mut slow = ResizeGateHarness::new(50, 50);
    slow.window_resized(narrow, false, start);
    slow.tick(start + WINDOW_RESIZE_QUIET);
    slow.window_resized(wide, false, start + Duration::from_secs(2));
    slow.tick(start + Duration::from_secs(2) + WINDOW_RESIZE_QUIET);
    assert_eq!(
        slow.requests,
        vec![grid_of(16, 50), grid_of(50, 50)],
        "the shell was left holding the width the pane no longer has"
    );
    assert_eq!(slow.conpty, grid_of(50, 50));
    assert_eq!(slow.grid, grid_of(50, 50));

    // ② The squeeze is over before the quiet window is.
    let mut quick = ResizeGateHarness::new(50, 50);
    quick.window_resized(narrow, false, start);
    quick.window_resized(wide, false, start + Duration::from_millis(50));
    quick.tick(start + Duration::from_secs(1));
    assert_eq!(
        quick.requests,
        Vec::new(),
        "a squeeze the child never heard about needs no undoing"
    );
    assert_eq!(quick.conpty, grid_of(50, 50));
    assert_eq!(quick.grid, grid_of(50, 50));
    // But it was a gesture, and it is over. The pane reflowed to sixteen columns and back on
    // the way, and only this release can close the transaction that opened.
    assert_eq!(
        quick.settlements, 1,
        "the end of the gesture is reported even though the child is not"
    );
    let deadline = quick
        .session
        .resize_finish_deadline()
        .expect("the settlement arms the deadline that closes the transaction");
    assert!(!quick.quiesce(deadline - Duration::from_millis(1)));
    assert!(
        quick.quiesce(deadline),
        "and the transaction closes at its own deadline"
    );
    assert!(!quick.session.resize_transaction_open());

    // ③ And the drag that wanders: narrow, narrower, back out, all inside one quiet window,
    // then held wide past it. The child hears the last word and nothing else.
    let mut wandering = ResizeGateHarness::new(50, 50);
    wandering.window_resized(narrow, false, start);
    wandering.window_resized(
        PhysicalSize::new(600, 1800),
        false,
        start + Duration::from_millis(30),
    );
    wandering.window_resized(
        PhysicalSize::new(1920, 1800),
        false,
        start + Duration::from_millis(60),
    );
    wandering.tick(start + Duration::from_millis(60) + WINDOW_RESIZE_QUIET);
    assert_eq!(wandering.requests, vec![grid_of(33, 50)]);
    assert_eq!(wandering.conpty, grid_of(33, 50));
}

/// RED — **a divider drag the system takes the pointer away from is over, and the resize it
/// was holding is released** (Codex review 2026-09-17).
///
/// The one way a gesture could still leave a pane unsettled. `divider_drag` is torn down by
/// the button coming up, by `Esc`, by a re-solve and by a blur, and capture loss is none of
/// them: Windows announces it with `WM_CAPTURECHANGED`, winit 0.30.13 answers that message by
/// zeroing its own capture count and emitting nothing, and a steal need not blur this window.
/// The flag then stays `Some` forever, `flush_pending_pty_resize` reads it as a hand still on
/// the geometry, and `service_pending_pty_resize` refuses both the release and a wake — so
/// every later resize of every pane of this window is held, unreleased and unsettled, for the
/// rest of its life.
///
/// Red gate: delete the `end_a_divider_drag_that_lost_its_pointer` line from
/// `flush_pending_pty_resize` — the last assertion names the release that never came. Phrase
/// the predicate as "this window holds the capture" instead of "the same window holds it as
/// held it at the press" and the macOS case cancels every divider drag on its first turn.
#[test]
fn a_divider_drag_that_loses_its_pointer_stops_holding_the_resize() {
    let held = Some(bt_platform::NativeWindow::stand_in(0x1234));
    assert!(
        divider_drag_still_holds_its_pointer(held, held),
        "an untouched capture is a hand still on the divider"
    );
    assert!(
        divider_drag_still_holds_its_pointer(None, None),
        "and on a platform with no per-thread capture to lose, nothing was taken away"
    );
    assert!(!divider_drag_still_holds_its_pointer(
        held,
        Some(bt_platform::NativeWindow::stand_in(0x4321))
    ));
    assert!(
        !divider_drag_still_holds_its_pointer(held, None),
        "and the shape the report is about: the system took it and told nobody"
    );

    // The recovery is read before the held-hand question it exists to keep honest.
    let flush = method_body("Runtime", "flush_pending_pty_resize");
    let recovery = flush
        .find("end_a_divider_drag_that_lost_its_pointer")
        .expect("the flush ends a gesture nobody is holding any more");
    let hand = flush
        .find("let hand_on_the_geometry")
        .expect("and then asks whether a hand is on the geometry");
    assert!(
        recovery < hand,
        "a stale divider drag has to be cleared before it is believed"
    );

    let start = Instant::now();
    let mut harness = ResizeGateHarness::new(50, 50);
    // A divider is being dragged, and this pane is being squeezed by it.
    harness.hand_down = true;
    harness.window_resized(PhysicalSize::new(1500, 1800), false, start);
    harness.tick(start + Duration::from_secs(5));
    assert_eq!(
        harness.requests,
        Vec::new(),
        "while a hand is on the divider the child hears nothing, which is the rule"
    );
    assert!(
        harness.session.resize_transaction_open(),
        "and the reflow that followed the hand has a transaction open"
    );

    // The pointer is taken away. No blur, no button-up: the only thing that changes is the
    // answer the recovery reads.
    harness.hand_down = divider_drag_still_holds_its_pointer(held, None);
    harness.tick(start + Duration::from_secs(5) + Duration::from_millis(1));
    assert_eq!(
        harness.requests,
        vec![grid_of(26, 50)],
        "the gesture is over, so the release it was holding is delivered"
    );
    let deadline = harness
        .session
        .resize_finish_deadline()
        .expect("and that release settles the transaction");
    assert!(harness.quiesce(deadline));
    assert!(
        !harness.session.resize_transaction_open(),
        "which is what lets this pane scan for formulas again"
    );
}

/// And a rectangle nobody is holding is unchanged by any of it: a preview opening, a tab
/// closing, an OS resize that arrives all at once still reach the child at the ordinary quiet
/// boundary.
///
/// Red gate: gate the release on the hand being *down* rather than up and this goes red.
#[test]
fn a_layout_change_no_hand_is_holding_still_reaches_the_shell_at_the_quiet_boundary() {
    let start = Instant::now();
    let mut harness = ResizeGateHarness::new(50, 50);
    harness.window_resized(PhysicalSize::new(1500, 1800), false, start);
    harness.tick(start + WINDOW_RESIZE_QUIET);
    assert_eq!(harness.requests, vec![grid_of(26, 50)]);
}

/// POLICY PIN (user ruling 2026-08-06, machine retired 2026-08-16): **typed input does not
/// hold a resize.** The local grid follows the drag in the same turn even while the shell is
/// holding an unsubmitted line, and ConPTY receives exactly the coalesced final size at the
/// ordinary quiet boundary — not one request per drag step, and not none at all.
///
/// This used to read the policy bit and assert it was `false`. The bit is gone, along with the
/// confirm-then-release machine behind it, so what is left is the pin on the behaviour that
/// actually ships: the ruling's own sentence, said about the only path there now is.
///
/// Red gate: restore a hold — make `plan_grid_change` withhold the reflow while
/// `typed_shell_input_live` — and the first assertion goes red naming the grid that did not
/// move. Drop the coalescer and the second goes red instead.
#[test]
fn a_drag_reflows_at_once_while_the_shell_holds_typed_input() {
    let start = Instant::now();
    let mut harness = ResizeGateHarness::new(100, 24);
    harness.feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07Get-ChildItem", start);
    assert!(harness.session.typed_shell_input_live());

    harness.drag_to(80, start);
    assert_eq!(
        harness.grid,
        grid_of(80, 24),
        "the local grid reflows even while the shell holds text"
    );
    harness.tick(start + WINDOW_RESIZE_QUIET - Duration::from_millis(1));
    assert!(
        harness.requests.is_empty(),
        "the 200 ms coalescer remains in force"
    );
    harness.tick(start + WINDOW_RESIZE_QUIET);
    assert_eq!(
        harness.requests,
        vec![grid_of(80, 24)],
        "the coalesced resize commits while input remains live"
    );
}

/// RED (T-RESTART-CWD round 2, finding 3) — **the session save writes the one ladder**, the
/// one every shell started in a pane's place reads.
///
/// MUTATION, observed red: write `working_directory().or(spawn_place)` inline again.
#[test]
fn the_session_save_writes_the_one_ladder() {
    let save = method_body("TabState", "term_leaf");
    assert!(
        save.contains(".and_then(LeafSession::place_for_a_new_shell)")
            && !save.contains("spawn_place"),
        "{save}"
    );
}

/// PIN — U8. A pane's FLIP starts on the box it left and ends on the box the
/// solver gave it, and it travels the whole way in between.
///
/// The two ends are the assertions that make it a FLIP rather than a
/// decoration: if `e = 0` does not reproduce the *before* rect exactly then
/// the first frame of the animation is a jump, which is the one thing the
/// technique exists to remove; and if `e = 1` is not the solver's own answer
/// then the pane has been left somewhere the layout does not think it is.
#[test]
fn a_pane_flip_starts_on_the_rect_it_left_and_lands_on_the_one_it_was_given() {
    let now = Instant::now();
    let before = [100.0, 50.0, 300.0, 250.0];
    let after = [200.0, 150.0, 600.0, 550.0];
    let flip = PaneFlip::displace(before, after, now, Motion::Full)
        .expect("a pane that halved its corner and doubled its size is displaced");

    let (start, moving) = flip.sample(now, Motion::Full);
    assert!(moving);
    let opened = start.applied_to(after);
    for channel in 0..4 {
        assert!(
            (opened[channel] - before[channel]).abs() < 1e-3,
            "the first frame is the box the pane left, not a jump: {opened:?} vs {before:?}"
        );
    }

    // Monotone toward the destination, in the corner and in the extent.
    let mut last = opened;
    for step in 1..=20 {
        let at = now + Duration::from_millis(step * 10);
        let (transform, _) = flip.sample(at, Motion::Full);
        let box_now = transform.applied_to(after);
        assert!(
            box_now[0] >= last[0] - 1e-3 && box_now[1] >= last[1] - 1e-3,
            "the corner only ever travels toward its destination: {box_now:?} after {last:?}"
        );
        assert!(
            box_now[2] - box_now[0] >= last[2] - last[0] - 1e-3,
            "and the pane only ever widens toward its destination"
        );
        last = box_now;
    }

    let (midway, _) = flip.sample(now + PANE_FLIP / 2, Motion::Full);
    assert!(
        midway.dx.abs() < 0.15 * 100.0 && midway.dy.abs() < 0.15 * 100.0,
        "`cubic-bezier(.2, 0, 0, 1)` leaves immediately: half the time is \
             more than 85% of the distance, which `ease` next door is not \
             ({midway:?})"
    );

    assert!(
        flip.sample(now + PANE_FLIP - Duration::from_millis(1), Motion::Full)
            .1,
        "still moving one millisecond short of the end"
    );
    let (landed, moving) = flip.sample(now + PANE_FLIP, Motion::Full);
    assert!(!moving);
    assert_eq!(
        landed,
        PaneTransform::IDENTITY,
        "at 200ms the pane is simply where the solver put it"
    );
    assert_eq!(landed.applied_to(after), after);
}

/// PIN — the two halves of a split run on **one** span, and it is the slow
/// one.
///
/// The mock-up writes `.2s` for the FLIP (6555) and `.18s` for the arriving
/// pane (6572), and this test used to hold both apart against being "quietly
/// rounded into" each other. §7.18 rounds them deliberately: the two are the
/// two halves of one split, they happen instead of each other on the very
/// same event, and a pane that arrived a fifth of a second before its
/// neighbour finished moving was one event drawn at two tempos.
#[test]
fn a_split_flips_and_arrives_on_the_one_slow_span() {
    assert_eq!(PANE_FLIP, bt_render::MOTION_SLOW);
    assert_eq!(PANE_FADE_IN, bt_render::MOTION_SLOW);
    assert_eq!(
        PANE_FLIP, PANE_FADE_IN,
        "one split, one span — see `bt_render::motion`"
    );
}

/// PIN — the pane FLIP and the tab FLIP share a curve and not a duration.
///
/// Two ways this gets "simplified", and they pull opposite ways. Declare a
/// second `[0.2, 0.0, 0.0, 1.0]` beside [`GRAB_EASE`] and the two drift the
/// day somebody tunes one of them. Fold the durations together and either
/// the strip goes sluggish or the split goes abrupt, depending on which
/// number survived — the mock-up writes `.2s` at 6555 and `.16s` at 6570 and
/// means both.
#[test]
fn the_pane_flip_and_the_tab_flip_share_a_curve_but_not_a_duration() {
    let now = Instant::now();
    let pane = PaneFlip::displace(
        [100.0, 0.0, 200.0, 100.0],
        [0.0, 0.0, 100.0, 100.0],
        now,
        Motion::Full,
    )
    .expect("a hundred physical pixels is a displacement");
    let mut tab = FlipTween::default();
    tab.displace(100.0, now, Motion::Full);

    // Same curve means the same fraction of the journey is left at the same
    // fraction of the span — whatever the spans happen to be.
    for tenth in 1..10 {
        let pane_left = pane.sample(now + PANE_FLIP * tenth / 10, Motion::Full).0.dx / 100.0;
        let tab_left = tab.sample(now + TAB_FLIP * tenth / 10, Motion::Full).0 / 100.0;
        assert!(
            (pane_left - tab_left).abs() < 1e-3,
            "at {tenth}/10 of its own span the pane has {pane_left} left and the \
                 tab {tab_left} — they are not running the same curve"
        );
    }
    assert_ne!(
        PANE_FLIP, TAB_FLIP,
        "a pane is half the window and a tab is a chip in a row; one number \
             cannot be both"
    );
}

/// PIN — P178. A pane that barely moved gets no animation at all, and each
/// of the four channels alone is enough to earn one.
///
/// `if (Math.abs(dx) < .5 && Math.abs(dy) < .5 && Math.abs(sx - 1) < .005 &&
/// Math.abs(sy - 1) < .005) return;` (mock-up 6580). Four terms, so four
/// assertions: delete any one of them from the condition and exactly one of
/// these goes red, which is the only way a conjunction can be pinned.
#[test]
fn a_pane_that_barely_moved_gets_no_tween_and_each_channel_alone_earns_one() {
    let now = Instant::now();
    let seat = [0.0, 0.0, 128.0, 128.0];

    assert!(
        PaneFlip::displace(seat, seat, now, Motion::Full).is_none(),
        "a pane the solver did not move is not animated at all"
    );
    assert!(
        PaneFlip::displace([0.49, 0.49, 128.49, 128.49], seat, now, Motion::Full).is_none(),
        "and neither is one that moved less than half a physical pixel"
    );
    assert!(
        PaneFlip::displace([0.0, 0.0, 128.5, 128.5], seat, now, Motion::Full).is_none(),
        "nor one whose extent changed by less than half a percent \
             (128.5/128 = 1.0039)"
    );

    assert!(
        PaneFlip::displace([0.5, 0.0, 128.5, 128.0], seat, now, Motion::Full).is_some(),
        "half a physical pixel of `dx` alone earns the animation — the \
             solver snaps seat boundaries to whole pixels, so this is the \
             smallest move there is"
    );
    assert!(
        PaneFlip::displace([0.0, 0.5, 128.0, 128.5], seat, now, Motion::Full).is_some(),
        "half a physical pixel of `dy` alone earns it"
    );
    assert!(
        PaneFlip::displace([0.0, 0.0, 129.0, 128.0], seat, now, Motion::Full).is_some(),
        "`sx` alone earns it: 129/128 = 1.0078, past the half-percent, with \
             the corner untouched"
    );
    assert!(
        PaneFlip::displace([0.0, 0.0, 128.0, 129.0], seat, now, Motion::Full).is_some(),
        "`sy` alone earns it, likewise"
    );
}

/// PIN — reduced motion puts a displaced pane where it belongs on the first
/// sample, and an arriving one at full opacity.
///
/// **Ruling**, and the same one [`reduced_motion_puts_a_displaced_tab_straight_into_its_slot`]
/// records: the mock-up writes these transitions from JavaScript, where no
/// `prefers-reduced-motion` block can reach them, so its silence is its
/// medium rather than a decision. Half the window changing shape is exactly
/// what the preference is about.
#[test]
fn reduced_motion_lands_a_pane_at_once_and_gives_an_arriving_one_no_fade() {
    let now = Instant::now();
    let after = [200.0, 150.0, 600.0, 550.0];
    let flip = PaneFlip::displace([100.0, 50.0, 300.0, 250.0], after, now, Motion::Reduced)
        .expect("the threshold is about geometry, not about preference");
    assert_eq!(
        flip.sample(now, Motion::Reduced),
        (PaneTransform::IDENTITY, false),
        "there is no first frame to travel from: the pane is where it belongs"
    );

    let mut motion = PaneMotion::default();
    let before = [(SeatId(1), [0.0, 0.0, 100.0, 100.0])];
    let arriving = [
        (SeatId(1), [0.0, 0.0, 50.0, 100.0]),
        (SeatId(2), [50.0, 0.0, 100.0, 100.0]),
    ];
    motion.begin(&before, &arriving, now, Motion::Reduced);
    assert_eq!(
        motion.opacity_of(SeatId(2), now, Motion::Reduced),
        1.0,
        "the new pane is simply there"
    );
    assert!(!motion.is_animating(now, Motion::Reduced));

    let mut full = PaneMotion::default();
    full.begin(&before, &arriving, now, Motion::Full);
    assert_eq!(
        full.opacity_of(SeatId(2), now, Motion::Full),
        0.0,
        "under Full it starts from nothing"
    );
    let opening = full.opacity_of(SeatId(2), now + Duration::from_millis(90), Motion::Full);
    assert!(opening > 0.0 && opening < 1.0, "mid-fade: {opening}");
    assert_eq!(
        full.opacity_of(SeatId(2), now + PANE_FADE_IN, Motion::Full),
        1.0,
        "and it is done at its own span, not the FLIP's"
    );
    assert_eq!(
        full.transform_of(SeatId(2), now, Motion::Full),
        PaneTransform::IDENTITY,
        "a pane with no history fades *instead of* sliding (mock-up 6573 returns)"
    );
}

/// PIN — a pane split again mid-flight starts from where it actually is.
///
/// `snapshotPanes` measures `getBoundingClientRect` (mock-up 6557-6562),
/// which includes the transform still running. Restart from the solver's
/// stale rect instead and the pane teleports to a box it never reached
/// before beginning its second journey — which is [`FlipTween::displace`]'s
/// argument, one dimension up.
#[test]
fn a_pane_displaced_again_mid_flight_starts_from_where_it_actually_is() {
    let now = Instant::now();
    let first = [0.0, 0.0, 100.0, 100.0];
    let mut motion = PaneMotion::default();
    motion.begin(
        &[(SeatId(1), [200.0, 0.0, 300.0, 100.0])],
        &[(SeatId(1), first)],
        now,
        Motion::Full,
    );

    let at = now + Duration::from_millis(80);
    let live = motion.snapshot(&[(SeatId(1), first)], at, Motion::Full);
    let [live_left, ..] = live[0].1;
    assert!(
        live_left > 1.0,
        "80ms into a 200ms flight the pane has not arrived ({live_left})"
    );

    let second = [0.0, 300.0, 100.0, 400.0];
    motion.begin(&live, &[(SeatId(1), second)], at, Motion::Full);
    let opened = motion
        .transform_of(SeatId(1), at, Motion::Full)
        .applied_to(second);
    for channel in 0..4 {
        assert!(
            (opened[channel] - live[0].1[channel]).abs() < 1e-3,
            "the second flight opens on the live box {:?}, not on the stale \
                 layout rect {first:?} — it got {opened:?}",
            live[0].1
        );
    }
    assert!(
        (opened[0] - first[0]).abs() > 1.0,
        "and the stale rect really is a different box, so this test can fail"
    );
}

/// PIN — a seat that left the layout leaves nothing behind.
///
/// The mock-up gives a departing pane no exit animation: `renderWithPaneFlip`
/// walks the panes that exist *after* the render (6564), and the closed one
/// is already out of the DOM. Only the survivors FLIP.
#[test]
fn a_seat_that_left_the_layout_leaves_no_tween_behind() {
    let now = Instant::now();
    let split = [
        (SeatId(1), [0.0, 0.0, 50.0, 100.0]),
        (SeatId(2), [50.0, 0.0, 100.0, 100.0]),
    ];
    let whole = [(SeatId(1), [0.0, 0.0, 100.0, 100.0])];
    let mut motion = PaneMotion::default();
    motion.begin(&[], &split, now, Motion::Full);
    assert!(motion.panes.iter().any(|pane| pane.seat == SeatId(2)));

    motion.begin(&split, &whole, now, Motion::Full);
    assert!(
        !motion.panes.iter().any(|pane| pane.seat == SeatId(2)),
        "the departed seat is not carried"
    );
    assert_eq!(
        motion.transform_of(SeatId(2), now, Motion::Full),
        PaneTransform::IDENTITY
    );
    assert_eq!(motion.opacity_of(SeatId(2), now, Motion::Full), 1.0);
    assert!(
        motion.is_animating(now, Motion::Full),
        "the survivor is still expanding into the space"
    );
}

/// PIN — the pane's frame debt is paid in drawn boxes, so the flight draws
/// every step it has and stops the moment it lands.
///
/// The failure this stands against is the one [`tab_owes_frame`] was written
/// for: "is anything moving?" is the right question for the deadline and the
/// wrong one for whether to draw, and the two part company on exactly the
/// frame motion stops. Ask the moving question and the pane is left stranded
/// one eased step short of the box the solver actually gave it.
#[test]
fn a_pane_s_frame_debt_is_paid_in_drawn_boxes() {
    let now = Instant::now();
    let layout = [(SeatId(1), [0.0, 0.0, 500.0, 500.0])];
    let mut motion = PaneMotion::default();
    motion.begin(
        &[(SeatId(1), [400.0, 0.0, 900.0, 500.0])],
        &layout,
        now,
        Motion::Full,
    );

    assert!(
        motion.settle_frame_debt(&layout, now, Motion::Full),
        "nothing of this layout has been drawn yet"
    );
    assert!(
        !motion.settle_frame_debt(&layout, now, Motion::Full),
        "and asking twice at the same instant does not owe a second present"
    );
    assert!(
        motion.settle_frame_debt(&layout, now + Duration::from_millis(100), Motion::Full),
        "mid-flight the pane owes a frame"
    );

    let end = now + PANE_FLIP;
    assert!(
        !motion.is_animating(end, Motion::Full),
        "nothing is moving on the frame the flight ends"
    );
    assert!(
        motion.settle_frame_debt(&layout, end, Motion::Full),
        "and it still owes that frame — it is the one that puts the pane in \
             the box the solver gave it"
    );
    assert!(
        !motion.settle_frame_debt(&layout, end + pace::DEFAULT_FRAME_INTERVAL, Motion::Full),
        "landed and drawn, it owes nothing"
    );

    // Finer than a whole physical pixel is finer than anything is drawn at,
    // so the long tail of `.2,0,0,1` must not owe a present per wake-up.
    let mut settled = PaneMotion::default();
    settled.begin(
        &[(SeatId(1), [400.0, 0.0, 900.0, 500.0])],
        &layout,
        now,
        Motion::Full,
    );
    let mut presents = 0_u32;
    let mut wakes = 0_u32;
    let mut at = now;
    loop {
        let moving = settled.is_animating(at, Motion::Full);
        if settled.settle_frame_debt(&layout, at, Motion::Full) {
            presents += 1;
        }
        wakes += 1;
        if !moving {
            break;
        }
        at += pace::DEFAULT_FRAME_INTERVAL;
    }
    assert!(
        wakes > presents,
        "every wake-up presented a frame ({presents} of {wakes}) — the debt is \
             measured on something finer than a pane is drawn at"
    );
    assert!(
        presents >= 5,
        "only {presents} frames of the flight were drawn — that is a jump \
             wearing an animation's clothes"
    );
}

/// PIN — the pane wake is `None` when nothing is moving and the strip's
/// absolute animation appointment while something is, on the same terms as
/// [`Runtime::strip_animation_work`].
///
/// `None` is the important half: it is what lets the loop fall back to
/// `ControlFlow::Wait` and the process go genuinely idle once the split has
/// settled.
#[test]
fn the_pane_wake_uses_one_absolute_appointment_only_while_something_is_moving() {
    let now = Instant::now();
    let next_animation_deadline = now + pace::DEFAULT_FRAME_INTERVAL;
    let wake = |panes: &PaneMotion, at: Instant, preference: Motion| {
        panes
            .is_animating(at, preference)
            .then_some(next_animation_deadline)
    };
    let mut motion = PaneMotion::default();
    assert_eq!(
        wake(&motion, now, Motion::Full),
        None,
        "a tab whose panes are all at rest asks for no wake-ups at all"
    );

    let before = [(SeatId(1), [0.0, 0.0, 100.0, 100.0])];
    let after = [
        (SeatId(1), [0.0, 0.0, 50.0, 100.0]),
        (SeatId(2), [50.0, 0.0, 100.0, 100.0]),
    ];
    motion.begin(&before, &after, now, Motion::Full);
    let first = wake(&motion, now, Motion::Full);
    let second = wake(&motion, now + PANE_FLIP / 2, Motion::Full);
    assert_eq!(first, Some(next_animation_deadline));
    assert_eq!(
        second, first,
        "an unchanged flight keeps the same absolute appointment when queried later"
    );
    assert_eq!(
        wake(&motion, now + PANE_FLIP, Motion::Full),
        None,
        "and once the split lands, the loop may sleep"
    );

    motion.retire(now + PANE_FLIP, Motion::Full);
    assert!(motion.panes.iter().all(|pane| pane.tween.is_none()));

    let mut reduced = PaneMotion::default();
    reduced.begin(&before, &after, now, Motion::Reduced);
    assert_eq!(
        wake(&reduced, now, Motion::Reduced),
        None,
        "reduced motion has no frames to ask for"
    );
}

/// PIN — U8. Reduced motion puts every pane on its final rectangle at once,
/// owes no frame after the first and asks for no wake-ups.
///
/// Verified rather than assumed: "there are no tweens under Reduced" is a
/// property of [`PaneFlip::displace`] storing no `started`, and the three
/// things that follow from it — identity transform, settled debt, no
/// animation liveness — are each a separate consumer that could have read the clock
/// for itself.
#[test]
fn reduced_motion_lands_every_pane_at_once_and_asks_for_no_frames() {
    let now = Instant::now();
    let (seats, before, after, survivor, arriving) = split_window(true);
    let mut motion = PaneMotion::default();
    motion.begin(
        &pane_rects_of(&before),
        &pane_rects_of(&after),
        now,
        Motion::Reduced,
    );

    for seat in [survivor, arriving] {
        assert_eq!(
            motion.transform_of(seat, now, Motion::Reduced),
            PaneTransform::IDENTITY,
            "seat {seat:?} is simply where the solver put it"
        );
    }
    let pane = pane_box_of(&after, survivor);
    let body =
        seats::pane_body_viewport(&seats, &after, survivor, 1.0).expect("the survivor has a body");
    let (content, clip) = animated_pane_viewports(
        body,
        pane,
        motion.transform_of(survivor, now, Motion::Reduced),
    );
    assert_eq!((content, clip), (body, body));

    assert!(!motion.is_animating(now, Motion::Reduced));
    assert!(
        motion.settle_frame_debt(&pane_rects_of(&after), now, Motion::Reduced),
        "the layout it landed on has still never been drawn"
    );
    assert!(
        !motion.settle_frame_debt(
            &pane_rects_of(&after),
            now + pace::DEFAULT_FRAME_INTERVAL,
            Motion::Reduced
        ),
        "and after that one frame it owes nothing at all"
    );
}

/// PIN — U8. The flight is drawn on every frame it has, including its last,
/// and is no longer live afterwards.
///
/// The two questions kept apart: [`PaneMotion::is_animating`] answers whether
/// the shared absolute animation appointment belongs in the wake fold and
/// [`PaneMotion::settle_frame_debt`] answers "draw this one". They part company
/// on exactly the frame the FLIP reaches identity — where nothing is moving any
/// more and the pane has still not been drawn in the box the solver gave it.
/// This walks the whole flight at the wake-up cadence the loop actually uses
/// and checks both ends.
#[test]
fn a_pane_flight_requests_a_redraw_on_every_frame_it_has_and_none_after_it_lands() {
    let now = Instant::now();
    let (_, before, after, survivor, _) = split_window(true);
    let mut motion = PaneMotion::default();
    motion.begin(
        &pane_rects_of(&before),
        &pane_rects_of(&after),
        now,
        Motion::Full,
    );

    let mut drawn = 0_u32;
    let mut at = now;
    loop {
        let moving = motion.is_animating(at, Motion::Full);
        if motion.settle_frame_debt(&pane_rects_of(&after), at, Motion::Full) {
            drawn += 1;
        }
        motion.retire(at, Motion::Full);
        if !moving {
            break;
        }
        at += pace::DEFAULT_FRAME_INTERVAL;
    }
    assert!(
        drawn >= 5,
        "only {drawn} frames of a 200ms flight were drawn — that is a jump \
             wearing an animation's clothes"
    );
    // The frame after motion stops is walked too, and it is the one this
    // loop exists to protect: a debt gated on "is it moving?" would have
    // returned before it, leaving the pane one eased step short of the box
    // the solver gave it — a permanent misalignment rather than a missed
    // frame of polish. What is asserted is the outcome rather than which
    // frame paid for it, because the curve's long tail legitimately settles
    // on the final *pixels* a wake-up or two before it settles on the final
    // floats, and re-presenting identical pixels is a present nobody asked
    // for.
    assert_eq!(
        motion
            .entry(survivor)
            .and_then(|pane| pane.last_drawn)
            .map(|drawn| drawn.box_px),
        Some(pane_box_of(&after, survivor).map(|edge| edge.round() as i32)),
        "the flight ends with the pane recorded as drawn in the box the \
             solver gave it"
    );
    assert!(
        !motion.is_animating(at, Motion::Full),
        "and once it is drawn the window may go genuinely idle"
    );
    assert!(
        !motion.settle_frame_debt(
            &pane_rects_of(&after),
            at + pace::DEFAULT_FRAME_INTERVAL,
            Motion::Full
        ),
        "landed and drawn, it owes nothing"
    );
    assert!(
        motion.panes.iter().all(|pane| pane.tween.is_none()),
        "and `retire` has dropped the tweens, so no rect from a layout two \
             splits ago is carried beside a seat id that may be reused"
    );
}

/// PIN — U8, P177. The arriving pane's veil is `--termbg` at `1 - opacity`,
/// over the arriving pane's own rectangle and over no other.
///
/// The ruling this stands on is written out at [`pane_fade_veil_layers`]: the
/// fade is a cover in the ground colour rather than a uniform alpha on the
/// pane's own draws, because `termbg·(1-a) + pane·a` over a `--termbg` ground
/// *is* CSS `opacity`, and because glyphon has no per-`TextArea` alpha to
/// carry the literal thing without rebuilding every glyph buffer per frame.
///
/// Three things are pinned and each fails a different mistake: the colour
/// (a veil in `--panel` would flash the gap colour over a pane that is not
/// being resized), the alpha's *direction* (`opacity` instead of
/// `1 - opacity` fades the pane out and then snaps it in), and the extent
/// (a veil over the survivor as well would dim half the window on every
/// split).
#[test]
fn an_arriving_pane_is_veiled_in_the_ground_colour_at_one_minus_its_opacity() {
    let now = Instant::now();
    let palette = bt_render::chrome_palette();
    let before = [(SeatId(1), [0.0, 0.0, 100.0, 100.0])];
    let after = [
        (SeatId(1), [0.0, 0.0, 50.0, 100.0]),
        (SeatId(2), [50.0, 0.0, 100.0, 100.0]),
    ];
    let mut motion = PaneMotion::default();
    motion.begin(&before, &after, now, Motion::Full);

    let layers = pane_fade_veil_layers(&motion, &after, now, Motion::Full, palette);
    let [layer] = layers.as_slice() else {
        panic!("one veil layer while one pane is arriving, got {layers:?}");
    };
    let [veil] = layer.quads.as_slice() else {
        panic!(
            "one fill, for the one pane that is arriving — the survivor FLIPs \
                 and the two are alternatives (mock-up 6573): {:?}",
            layer.quads
        );
    };
    assert_eq!(
        veil.color, palette.seat_body,
        "the veil is the ground the pane fades up from — `--termbg`, which is \
             what is under a pane's box"
    );
    assert_ne!(
        palette.seat_body, palette.title_bar,
        "and the two really are different colours, so the assertion above can fail"
    );
    assert_eq!(
        veil.alpha, 1.0,
        "at t=0 the arriving pane is not there at all"
    );
    assert_eq!(
        veil.rect, after[1].1,
        "over the arriving seat's own solved rectangle"
    );
    assert_ne!(
        after[1].1, after[0].1,
        "and that rectangle really is not the survivor's"
    );

    for ms in [30_u64, 90, 150] {
        let at = now + Duration::from_millis(ms);
        let layers = pane_fade_veil_layers(&motion, &after, at, Motion::Full, palette);
        let veil = layers[0].quads[0];
        let want = 1.0 - motion.opacity_of(SeatId(2), at, Motion::Full);
        assert!(
            (veil.alpha - want).abs() < 1e-6,
            "{ms}ms in the veil is {} where the fade says {want}",
            veil.alpha
        );
        assert!(
            veil.alpha > 0.0 && veil.alpha < 1.0,
            "and {ms}ms of a 180ms fade is genuinely mid-way: {}",
            veil.alpha
        );
        assert_eq!(veil.rect, after[1].1);
    }

    assert!(
        pane_fade_veil_layers(&motion, &after, now + PANE_FADE_IN, Motion::Full, palette)
            .is_empty(),
        "at 180ms the pane is simply there, and an empty layer would still cost \
             a pass through three channels to draw nothing"
    );
}

/// PIN — U8, P177. A pane that FLIPs wears no veil, and under reduced motion
/// nothing wears one at all.
///
/// The first half is the mock-up's early `return` (6573) read as a fact about
/// drawing: FLIP and fade are alternatives, so a survivor that is sliding into
/// its new box must not also be dimmed on the way. The second is the same
/// answer [`RevealTween::retarget`] already gives — under `Reduced` the
/// arriving pane is simply *there* — asserted at the drawing rather than
/// inferred from the tween, because a veil is a separate consumer that could
/// have read the clock for itself.
#[test]
fn a_flipping_pane_wears_no_veil_and_reduced_motion_hangs_none_at_all() {
    let now = Instant::now();
    let palette = bt_render::chrome_palette();
    let before = [(SeatId(1), [0.0, 0.0, 100.0, 100.0])];
    let after = [
        (SeatId(1), [0.0, 0.0, 50.0, 100.0]),
        (SeatId(2), [50.0, 0.0, 100.0, 100.0]),
    ];

    let mut full = PaneMotion::default();
    full.begin(&before, &after, now, Motion::Full);
    assert!(
        full.is_animating(now, Motion::Full),
        "the survivor is mid-FLIP, or this test proves nothing"
    );
    let mut at = now;
    loop {
        let moving = full.is_animating(at, Motion::Full);
        for layer in pane_fade_veil_layers(&full, &after, at, Motion::Full, palette) {
            for veil in layer.quads {
                assert_ne!(
                    veil.rect, after[0].1,
                    "the survivor is FLIPping and a FLIP is never also a fade"
                );
            }
        }
        if !moving {
            break;
        }
        at += pace::DEFAULT_FRAME_INTERVAL;
    }

    let mut reduced = PaneMotion::default();
    reduced.begin(&before, &after, now, Motion::Reduced);
    for step in 0..16 {
        let at = now + pace::DEFAULT_FRAME_INTERVAL * step;
        assert!(
            pane_fade_veil_layers(&reduced, &after, at, Motion::Reduced, palette).is_empty(),
            "reduced motion has no fade to veil, on frame {step}"
        );
    }
}

/// PIN — U8, P177. A pane that only fades owes a frame on every frame of its
/// 180ms and none after.
///
/// The fade has no geometry at all — [`PaneTween::FadeIn`] reports the
/// identity transform — so the only thing that changes across it is the
/// opacity, and this is what pins that [`PaneDrawn`] carries it. Drop the
/// opacity from that record and the box never moves, the debt is never owed,
/// and the arriving pane is drawn once at nothing and never again: a pane that
/// simply does not appear.
#[test]
fn an_arriving_panes_fade_owes_a_frame_while_it_runs_and_none_after_it_lands() {
    let now = Instant::now();
    let arriving = [(SeatId(2), [50.0, 0.0, 100.0, 100.0])];
    let mut motion = PaneMotion::default();
    motion.begin(&[], &arriving, now, Motion::Full);
    assert_eq!(
        motion.transform_of(SeatId(2), now, Motion::Full),
        PaneTransform::IDENTITY,
        "a fade moves no box, so every frame it owes is owed for its opacity"
    );

    let mut presents = 0_u32;
    let mut at = now;
    loop {
        let moving = motion.is_animating(at, Motion::Full);
        if motion.settle_frame_debt(&arriving, at, Motion::Full) {
            presents += 1;
        }
        if !moving {
            break;
        }
        at += pace::DEFAULT_FRAME_INTERVAL;
    }
    assert!(
        presents >= 5,
        "only {presents} frames of a 180ms fade were drawn — that is an \
             appearance wearing a fade's clothes"
    );
    assert!(
        !motion.settle_frame_debt(&arriving, at + pace::DEFAULT_FRAME_INTERVAL, Motion::Full),
        "and once it is fully there it owes nothing"
    );
}

/// PIN — U8. Switching tabs clears the motion: you do not glide a layout you
/// were not looking at.
///
/// The two lines `activate_tab` runs, and both of them matter. Dropping the
/// tweens is the obvious half; adopting the arriving tab's own revision is
/// the half that is easy to forget, and the red gate at the foot is what
/// this pin exists for — leave the revision behind and the first commit in
/// the tab you switched to animates a change that happened in a *different*
/// tab, from rectangles nobody has ever seen.
///
/// (Stated against the state `activate_tab` writes rather than against a
/// `Runtime`, which needs a window and a GPU device to exist.)
#[test]
fn switching_tabs_drops_the_flight_and_adopts_the_arriving_tabs_own_revision() {
    let now = Instant::now();
    let (leaving, before, after, survivor, _) = split_window(true);
    let mut motion = PaneMotion::default();
    motion.begin(
        &pane_rects_of(&before),
        &pane_rects_of(&after),
        now,
        Motion::Full,
    );
    assert!(
        motion.is_animating(now, Motion::Full),
        "the tab being left is mid-flight"
    );
    // The arriving tab, which was split twice while nobody was looking.
    let metrics = seats::seat_metrics(1_000);
    let mut arriving_tab = seats::Seats::lone_terminal();
    let root = arriving_tab.identity();
    let second = arriving_tab
        .split_terminal(&metrics, root, bt_layout::Axis::Row, false)
        .expect("it divides");
    arriving_tab
        .split_terminal(&metrics, second, bt_layout::Axis::Col, false)
        .expect("and divides again");

    // Exactly what `activate_tab` writes.
    motion = PaneMotion::default();
    let revision = arriving_tab.structure_revision();

    assert!(!motion.is_animating(now, Motion::Full));
    assert_eq!(
        motion.transform_of(survivor, now, Motion::Full),
        PaneTransform::IDENTITY,
        "nothing of the tab you left is still moving"
    );
    assert_eq!(
        revision,
        arriving_tab.structure_revision(),
        "the gate is closed on arrival: the next commit in this tab compares \
             equal and leaves the motion alone"
    );
    assert_ne!(
        leaving.structure_revision(),
        arriving_tab.structure_revision(),
        "and the two counters really are different, so carrying the old one \
             across would have opened the gate on the very next commit"
    );
}

/// **§7.1.6k — the end of a tree is the trailing side, on the axis the
/// settings dialog names.**
///
/// Red gate: read the setting a second time here instead of going through
/// [`split_axis`] and `Split direction` stops governing this split alone;
/// answer `Left`/`Top` and "追加为树末尾" puts the arriving pane in front of
/// everything it was appended to.
#[test]
fn a_pane_appended_to_a_tree_joins_at_its_trailing_end() {
    use bt_persist::SplitDirectionV1;
    assert_eq!(trailing_edge(Axis::Row), seats::DropEdge::Right);
    assert_eq!(trailing_edge(Axis::Col), seats::DropEdge::Bottom);
    for measured in [Axis::Row, Axis::Col] {
        assert_eq!(
            trailing_edge(split_axis(SplitDirectionV1::Right, measured)),
            seats::DropEdge::Right,
            "the setting outranks the measurement, as it does everywhere else"
        );
        assert_eq!(
            trailing_edge(split_axis(SplitDirectionV1::Down, measured)),
            seats::DropEdge::Bottom
        );
        assert_eq!(
            trailing_edge(split_axis(SplitDirectionV1::Auto, measured)),
            trailing_edge(measured),
            "and `Auto` is the box's own longer side"
        );
    }
    assert_eq!(
        auto_split_axis(1_600, 900),
        Axis::Row,
        "an ordinary window is wider than it is tall, so a pane appended to \
             one arrives on the right"
    );
}

/// PIN (P123) — **gate ① is about the LAST preview pane and nothing else.**
///
/// "The pool outlives any ONE pane; only the LAST preview pane's close would
/// strand it." A window that asked every time a pane closed would be asking
/// about buffers still on screen, and a question you can answer by looking is
/// a question that trains you to dismiss the ones you cannot.
///
/// Mutation: drop the `previews_in_tab == 1` clause and the second assertion
/// goes red — every closed pane raises the gate; drop `closing_a_preview` and
/// the third does, and closing a *terminal* asks about a preview.
#[test]
fn only_the_last_preview_pane_strands_the_pool() {
    assert!(closing_this_pane_strands_the_pool(true, 1));
    assert!(
        !closing_this_pane_strands_the_pool(true, 2),
        "one of two previews leaves the pool on screen"
    );
    assert!(
        !closing_this_pane_strands_the_pool(false, 1),
        "a terminal closing beside a preview strands nothing"
    );
    assert!(!closing_this_pane_strands_the_pool(false, 0));
}

/// RED (owner's ruling 2026-10-04, T-STRIP-HOVER-THROUGH follow-up) — **the
/// search capsule is above the notice strip, on the glass and to the pointer,
/// and the two orders are one list.**
///
/// A surface the reader summoned (Ctrl+F) outranks a notice nobody asked for.
/// `IN_PANE_SURFACES_TOP_FIRST` is read by the paint (`OverlayStack::flattened`,
/// bottom first) and by the router (`pointer_target_at`, top first); the hover
/// and the press of every button read the router. The capsule hangs from the
/// seat's own top, so on a pane wearing a strip the two meet.
///
/// Red gates: put the strip first in the list, or paint the two bands by hand,
/// and the paint assertions fail; ask `drive_notice_hover` before
/// `drive_search_hover` and the hover assertion fails; take the in-pane press
/// below the chrome router, or answer the strip and the capsule by doors of
/// their own, and the click assertions fail.
#[test]
fn the_capsule_is_above_the_strip_for_the_paint_the_hover_and_the_press() {
    assert_eq!(
        IN_PANE_SURFACES_TOP_FIRST,
        [InPaneSurface::SearchCapsule, InPaneSurface::NoticeStrip],
        "the capsule the reader summoned is the top surface inside a pane"
    );
    // The paint, run: the strip is laid down first and the capsule over it.
    let layer = |opacity| marks::OverlayLayer {
        opacity,
        ..marks::OverlayLayer::default()
    };
    let stack = OverlayStack {
        search: vec![layer(0.25)].into(),
        pane_notices: vec![layer(0.5)].into(),
        ..OverlayStack::default()
    };
    let painted: Vec<f32> = stack
        .flattened()
        .layers
        .iter()
        .map(|layer| layer.opacity)
        .collect();
    assert_eq!(
        painted,
        [0.5, 0.25],
        "paint: the strip, then the capsule over it"
    );
    let paint = squeezed(item_body(&ItemQuery::method("OverlayStack", "flattened")));
    assert!(
        paint.contains("IN_PANE_SURFACES_TOP_FIRST.iter().rev()"),
        "paint: the two bands are placed by the one list, bottom first"
    );
    // The router reads the same list, top first, between the floats and the
    // docked chrome.
    let router = squeezed_body("Runtime", "pointer_target_at");
    let float = router
        .find("self.float_hit_at(position)")
        .expect("floats first");
    let in_pane = router
        .find("forsurfaceinIN_PANE_SURFACES_TOP_FIRST")
        .expect("pointer: the router walks the one list");
    let docked = router
        .find("self.docked_chrome_target_at(position)")
        .expect("and the docked chrome last");
    assert!(
        float < in_pane && in_pane < docked,
        "floats, then the in-pane surfaces, then the chrome"
    );
    // Hover.
    let moved = squeezed_body("Runtime", "pointer_moved");
    let capsule = moved
        .find("self.drive_search_hover(")
        .expect("the capsule's hover is driven on every move");
    let strip = moved
        .find("self.drive_notice_hover(")
        .expect("and the strip's");
    assert!(
        capsule < strip,
        "hover: the capsule, which is on top, is asked first"
    );
    assert!(
        moved[strip..]
            .starts_with("self.drive_notice_hover((self.window.mouse_route.is_none()&&!on_search)"),
        "and a hand the capsule has claimed lights nothing on the strip under it"
    );
    for (gesture, door) in [
        ("drive_search_hover", "self.search_at("),
        ("press_search", "self.search_at("),
        ("drive_notice_hover", "self.notice_at("),
        ("press_notice", "self.notice_at("),
    ] {
        assert!(
            squeezed_body("Runtime", gesture).contains(door),
            "`{gesture}` reads the router's answer"
        );
    }
    // Click: one door for both surfaces and every button, above the pane.
    let press = squeezed_body("Runtime", "mouse_input");
    let gate = press
        .find("self.press_in_pane_surface(button,position)?")
        .expect("click: the in-pane surfaces take their presses through one door");
    for below in [
        "self.point_is_on_the_web_page(position)",
        "self.chrome_mouse_input(state,button,position)?",
        "self.preview_rendered_surface_at(position)",
    ] {
        assert!(
            press[gate..].contains(below),
            "click: `{below}` is asked below the in-pane surfaces, never above them"
        );
    }
    let door = squeezed_body("Runtime", "press_in_pane_surface");
    assert!(
        door.contains("self.in_pane_surface_at(position)")
            && door.contains("InPaneSurface::SearchCapsule=>self.press_search(position)?")
            && door.contains("InPaneSurface::NoticeStrip=>self.press_notice(position)?"),
        "click: the door asks the router which surface, and hands the left button to it"
    );
    assert_eq!(
        reader_names(&calls_of("Runtime", "press_search")),
        ["press_in_pane_surface"],
        "the capsule's press has one caller"
    );
    assert_eq!(
        reader_names(&calls_of("Runtime", "press_notice")),
        ["press_in_pane_surface"],
        "and so does the strip's"
    );
}

/// PIN (**one hover-open path, and the press still goes straight through**)
/// — user ruling, 2026-09-10.
///
/// The ticket's own words: not a second clock. The pill registers with the
/// clock the chevrons already run — one `ChevronGates::observe` takes all
/// three buttons' states, one `advance_chevrons` reads all three — and the
/// rest, when it matures, opens the menu through
/// [`Runtime::open_preview_rail_menu`], which is the very function a press
/// on the pill calls. One menu, one anchor, two doors.
///
/// Read as **text**, for [`both_pointer_doors_tell_the_chevron_clocks_where_
/// the_hand_is`]' reason and only that reason: what this guards against is a
/// *second implementation*, and a second implementation that agrees today
/// cannot be driven into disagreeing by any state machine.
///
/// Red gate: give the pill its own gate field of its own type, its own
/// `observe` call, or its own menu-raising body, and the matching assertion
/// fails by name.
#[test]
fn the_rails_open_and_the_chevrons_share_one_hover_open_path() {
    let body = |name: &str| method_body("Runtime", name);
    // A field's declaration begins at its first attribute, and a `///` comment
    // is one — so this is the prose above the field and then the field. The
    // claim is about the field, so it is asked of the end of it.
    assert!(
        field_declaration("ChevronGates", "rail").ends_with("rail: profiles::ChevronGate"),
        "the pill's clock is the chevron's own type — a second kind of clock \
             is a second policy however equal its constants are today"
    );
    assert!(
        body("observe_chevrons").contains("(rail_where, rail_owner_open),"),
        "the pill is one of the states the one `observe` is handed, so a \
             caller cannot tell two of these buttons where the hand is and \
             forget the third"
    );
    assert!(
        body("advance_chevrons").contains("self.window.chevrons.rail.due(now)"),
        "and one function reads all three matured clocks"
    );
    assert!(
        body("advance_chevrons").contains("self.open_preview_rail_menu(surface)?"),
        "a matured rest raises the menu through the pill's own door, so the \
             menu a hand rests open is the menu a hand clicks open"
    );
    assert!(
        body("press_preview_rail").contains("self.open_preview_rail_menu(surface)"),
        "and the press is still a press: a click on a menu-opener opens it \
             at once and is never made to wait for a clock"
    );
}

/// PIN (user ruling 2026-08-25, B9) — **`Move to window` walks the drag's
/// road, and the ring is drawn by the window it is about.**
///
/// Two claims, both about *where* a thing is decided rather than about what
/// it does:
///
/// * the row records a [`DragHandover`] and lets `about_to_wait` spend it,
///   which is the errand F2 already wrote for a pane let go over another
///   window — so the promotion, the transfer, the refusal card and the
///   emptied source tab are one implementation and not two;
/// * the hovered window's mark is written on the *application* and drawn by
///   the window it names, because a window cannot draw a ring on its
///   neighbour and the pointer is never in the window being marked.
///
/// Red gate: give the row its own transfer and the first assertion names the
/// door that went its own way; draw the ring from the hovering window and
/// the third finds the mark on the wrong glass.
#[test]
fn the_third_exit_is_the_drags_own_errand_and_the_ring_is_the_targets_own_mark() {
    let body = |name: &str| method_body("Runtime", name);
    let row = body("move_pane_to_window");
    assert!(
        row.contains("self.app.pending_handover = Some(DragHandover {"),
        "the row writes the drag's errand rather than moving a tab itself"
    );
    assert!(
        !row.contains("transfer_tab"),
        "and it never reaches for the transfer directly: only `FolioApp` can \
             see two windows, and `settle_drag_handover` is where it does"
    );
    let ring = body("window_ring_layer");
    assert!(
        ring.contains("self.app.window_ring != Some(self.window_id())"),
        "the ring is drawn by the window it is about, off the application's \
             own aim"
    );
    assert!(
        body("aim_at_window").contains("self.app.window_ring = window;"),
        "and the aim is written on the application, because the window that \
             draws it is not the window the pointer is in"
    );
}

/// PIN — **the `Split direction` setting decides every split that has no
/// direction of its own, and `Auto` is the pane's own measurement.**
///
/// The `auto` argument is the answer the solver gave for *this* pane, so the
/// two non-auto arms are asserted to ignore it: a `Right` that quietly fell
/// back to the longer edge on a tall pane would be a setting that works only
/// where it was tested.
#[test]
fn a_split_with_no_direction_of_its_own_follows_the_setting() {
    use bt_persist::SplitDirectionV1;
    for measured in [Axis::Row, Axis::Col] {
        assert_eq!(
            split_axis(SplitDirectionV1::Auto, measured),
            measured,
            "Auto is whatever the pane's longer side turned out to be"
        );
        assert_eq!(split_axis(SplitDirectionV1::Right, measured), Axis::Row);
        assert_eq!(split_axis(SplitDirectionV1::Down, measured), Axis::Col);
    }
}

/// **The box is a state, and under reduced motion it is nothing else.**
///
/// The mock-up gives `#dock-preview` one transition and one only — `opacity
/// .1s` — and the geometry transitions beside it are unreachable by
/// construction: the box's rectangle is a function of the promise
/// (`zone:fits`), so any move of the box is a change of the promise, and
/// `promise()` puts `.snap` on before the new geometry lands. A glide could
/// only run while the answer stayed the same and the box moved anyway, which
/// within one drag cannot happen. So the box snaps, always — which is M148
/// satisfied by an implementation that has no way to lag.
///
/// What is left is the fade, and this is its span and its reduced-motion
/// answer. It is the archive's **fast** rung and the pin's is the base one:
/// a box that says "let go here" owes only to be there, while a control
/// widening open is one interaction. Borrowing the pin's would be the wrong
/// number arrived at silently.
#[test]
fn the_dock_box_fades_over_the_fast_span_and_not_at_all_reduced() {
    assert_eq!(DOCK_PREVIEW_FADE, bt_render::MOTION_FAST);
    assert_ne!(
        DOCK_PREVIEW_FADE,
        Duration::from_millis(bt_render::WINDOW_TAB_PIN_REVEAL_MS),
        "the dock box does not fade on the pin's clock"
    );

    let now = Instant::now();
    let mut reduced = RevealTween::over(DOCK_PREVIEW_FADE);
    reduced.retarget(1.0, now, Motion::Reduced);
    assert_eq!(
        reduced.sample(now, Motion::Reduced),
        (1.0, false),
        "reduced motion makes the box a state: it is there, and nothing moved"
    );

    let mut full = RevealTween::over(DOCK_PREVIEW_FADE);
    full.retarget(1.0, now, Motion::Full);
    let (opening, moving) = full.sample(now + Duration::from_millis(20), Motion::Full);
    assert!(
        moving && opening > 0.0 && opening < 1.0,
        "mid-fade: {opening}"
    );
    assert_eq!(
        full.sample(now + DOCK_PREVIEW_FADE, Motion::Full),
        (1.0, false),
        "and it is done at its own span, not before and not after"
    );
}

/// PIN — the leaf a payload lands as is the leaf its own kind names.
///
/// A file splits out a **preview** and a folder splits out a **files**
/// column, and getting this backwards is the whole gesture inverted: the
/// edge drop would open a directory in a page and a file in a tree.
///
/// Mutation: swap the two arms of `leaf_kind` — every edge drop lands the
/// wrong kind of pane, and both assertions fail.
#[test]
fn a_file_lands_as_a_page_and_a_folder_lands_as_a_tree() {
    assert_eq!(
        row_arrival_seat(RowPayloadKind::File).kind,
        bt_layout::SeatKind::Preview
    );
    assert_eq!(
        row_arrival_seat(RowPayloadKind::Folder).kind,
        bt_layout::SeatKind::Files
    );
    assert_eq!(
        row_arrival_seat(RowPayloadKind::Folder).fixed_extent,
        None,
        "F75: a column arriving fresh has no width to bring, so it opens at its kind's"
    );
}

/// PIN (W2 slice 5) - **one seat shows one thing, in both directions.**
///
/// Found on the machine and not by reading: double-click `page.html` in the
/// files column, then double-click `notes.md`, and the head, the foot and
/// the tab name all said `notes.md` while the browser was still on the glass
/// in front of the document (`shots/column-5-md.png`, before this). The
/// half that had always been there is `open_web_page_on`'s - a page landing
/// stops the document under it being pointed at - and until this slice
/// nothing could travel the other way, because the only door into a page was
/// `BT_WEB_DEV`.
///
/// The third case is the one that makes this a predicate rather than an
/// `is_some()`: between a page being asked for and its first commit the pane
/// points at nothing at all, on purpose, and reading that as a replacement
/// would close every page a frame after it opened.
#[test]
fn a_page_leaves_the_seat_that_stopped_showing_it() {
    assert!(
        !a_page_was_replaced(None, None),
        "a page that has been asked for and has not committed yet is not a \
             page somebody replaced"
    );
    assert!(!a_page_was_replaced(
        None,
        Some(&preview::PreviewSource::Web(
            "http://localhost:5173/app".into()
        ))
    ));
    assert!(
        a_page_was_replaced(
            None,
            Some(&preview::PreviewSource::file(
                r"D:
otes.md"
            ))
        ),
        "a document landed on the seat"
    );
    assert!(
        a_page_was_replaced(Some(Path::new(r"D:\shots.png")), None),
        "and so did a picture, which has no buffer to be found by"
    );
}

/// RED (A1d, row 12) — **each leaf's resize is one admitted `PtyResize`, and a refused one takes
/// the error road a failed resize takes.**
///
/// Two real shells, resized through the real `commit_leaf_resize` on a thread entered as the
/// window thread with its loop running: two admissions, one per leaf, in order — the flush that
/// walks the leaves is not admitted, each leaf's `ResizePseudoConsole` is. Then a third shell on
/// a window thread still in `Starting`, where the door is not admitted: the commit answers the
/// resize's own error, with the refusal named inside it.
///
/// MUTATION: give `doors::PtyResize` the `Starting` phase and the third commit succeeds; admit
/// anything else inside `commit_leaf_resize` and the admission list names it.
#[test]
fn each_leafs_resize_is_one_admission_and_a_refused_one_is_a_failed_resize() {
    fn commit(session: &mut DualPlaneSession, pty: &mut PtySession) -> Result<LeafResizeCommit> {
        let mut pending = false;
        commit_leaf_resize(
            session,
            Some(pty),
            ResizeReanchor {
                pending: &mut pending,
                integration: profiles::Integration::None,
            },
            ReleaseGrids {
                local: grid_of(80, 24),
                conpty: grid_of(80, 24),
                next: grid_of(60, 24),
            },
            PhysicalSize::new(480, 600),
            Instant::now(),
        )
    }
    fn a_leaf() -> (DualPlaneSession, bt_pty::test_shell::TestShell) {
        let size = PtySize::cells(
            std::num::NonZeroU16::new(80).unwrap(),
            std::num::NonZeroU16::new(24).unwrap(),
        );
        let pty = bt_pty::test_shell::TestShell::spawn_default(size).expect("a real shell");
        (DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24)), pty)
    }
    on_the_window_thread();
    let (mut first, mut first_pty) = a_leaf();
    let (mut second, mut second_pty) = a_leaf();
    let _ = crate::hang_watch::admissions_on_this_thread();
    commit(&mut first, first_pty.session_mut()).expect("the first leaf's child hears its size");
    commit(&mut second, second_pty.session_mut()).expect("and the second's");
    assert_eq!(
        crate::hang_watch::admissions_on_this_thread(),
        ["PtyResize", "PtyResize"],
        "one admission per leaf, and nothing else admitted on the way"
    );
    std::thread::spawn(|| {
        assert!(bt_platform::admission::enter_window_thread());
        let (mut session, mut pty) = a_leaf();
        let refused =
            commit(&mut session, pty.session_mut()).expect_err("not admitted before the loop");
        let said = format!("{refused:#}");
        assert!(
            said.contains("commit a coalesced final ConPTY resize")
                && said.contains("the PtyResize door was refused"),
            "a refused resize is the resize's own error, and says why: {said}"
        );
    })
    .join()
    .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}
