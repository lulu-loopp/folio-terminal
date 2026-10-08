//! **`profiles`, as the application drives it.** Tests whose first assertion is about
//! `profiles`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    BOTH_NOTIFICATION_ROWS_ON, POWERSHELL_PROMPT, SHARPEN_CONTENT, SHARPEN_NATIVE, THREE_LINES,
    TwoPaneHarness, a_page_that_wants_a_sharper_picture, answer, chevron_button, cross_metrics,
    cross_solve, cross_tab, free_fn_body, grid_of, hand_leaves, leaf_saying, leaf_says,
    method_body, on_a_screen, paste_leaf, paste_tab, paste_text_into, peek_open, request_attention,
    resolve_sharpening_page, ring, ringing_tab, saved_row_of_two, staged_bytes_sent, tab_holding,
    the_three_chevrons,
};

// ── the overlay's z-order (user ruling 2026-08-12) ──────────────────────

/// PIN — **the modal family covers the floating window.**
///
/// The user's report, as an order: a pinned files window was drawn over the
/// settings panel and hid its right-hand side. The cause was not a rule
/// anywhere — it was that the modal chain is a big `if/else if` wanting the
/// renderer, so it was assembled first, and in a list of `extend` calls
/// "first" silently means "underneath".
///
/// Asserted as the whole order rather than as the one pair that was wrong,
/// because this is the fourth cross-channel z-order bug in this window and
/// every one of them was a pair nobody had written down. A list that can be
/// read is the fix; this test is what makes it the list that is *used*.
///
/// Mutation: swap `float` and `modal` in [`OverlayStack::flattened`].
#[test]
fn the_modal_family_covers_the_float_and_the_tip_covers_them_both() {
    // One marker quad per family, carrying its own name in a colour so the
    // flattened order reads back as the families in order.
    let mark = |tag: u8| {
        marks::Band::from(vec![marks::OverlayLayer {
            quads: vec![bt_render::OverlayQuad {
                rect: [0.0, 0.0, 1.0, 1.0],
                color: [tag, 0, 0],
                alpha: 1.0,
            }],
            ..marks::OverlayLayer::default()
        }])
    };
    let stack = OverlayStack {
        preview_bars: mark(0),
        // 24 and not 23, for the reason written on `card_hint` below: a
        // marker shared by two families makes this whole assertion pass
        // while the two swap places, and this band arrived on a later day
        // than the Cards bubble did.
        video_bars: mark(24),
        terminal_bars: mark(16),
        command_rail: mark(13),
        // 26 and not 25: every number below is spoken for, and a marker
        // shared by two families would make this whole assertion pass while
        // the two swapped places.
        formula_tools: mark(26),
        rail: mark(1),
        flight: mark(19),
        ground: mark(2),
        search: mark(14),
        pane_notices: mark(17),
        web_sheet: mark(18),
        layout_peek: mark(3),
        float: mark(4),
        modal: mark(5),
        file_menu: mark(6),
        pane_menu: mark(7),
        git_menu: mark(12),
        term_menu: mark(15),
        tab_menu: mark(22),
        // 25 and not 24: 23 and 24 are spoken for by the Cards bubble and
        // the video bars, and a marker shared by two families would make
        // this whole assertion pass while the two swapped places.
        palette: mark(25),
        toast: mark(8),
        key_hint: mark(20),
        // 23 and not 22: the tab menu and the Cards bubble were written on
        // two branches on the same day and both reached for the next free
        // number. A marker shared by two families would make this whole
        // assertion pass while the two swapped places.
        card_hint: mark(23),
        tooltip: mark(9),
        file_peek: mark(10),
        drag_ghost: mark(11),
        window_ring: mark(21),
    };
    let order: Vec<u8> = stack
        .flattened()
        .layers
        .iter()
        .map(|layer| layer.quads[0].color[0])
        .collect();
    assert_eq!(
        order,
        vec![
            0, 24, 16, 13, 26, 1, 19, 2, 17, 14, 18, 3, 4, 5, 6, 7, 12, 15, 22, 25, 8, 20, 23, 9,
            10, 11, 21
        ],
        "bottom to top: pane bars, video bars, terminal thumbs, command rails, formula marks, \
             rail, flight, ground, integration strips, search capsule (owner's ruling 2026-10-04), download sheet, schematic, \
             float, modal, file menu, pane menu, git menu, terminal menu, tab menu, command \
             palette, notices, key hint, Cards bubble, tip, glance, ghost, window ring"
    );
    let at = |tag: u8| {
        order
            .iter()
            .position(|found| *found == tag)
            .expect("every family is in the stack")
    };
    // The three that the report and the ruling are actually about, spelled
    // out so a future reordering fails with the reason rather than with a
    // vector literal.
    assert!(
        at(5) > at(4),
        "the settings panel is painted over the floating window, not under it"
    );
    // §7.1.6b″, and the pair is the whole ruling: a card in flight covers
    // the column it came out of, and every surface you *summoned* still
    // covers it. Written as two comparisons rather than trusted to the
    // vector above, because "highest of its neighbours" and "highest in the
    // window" are the two readings the ruling had to choose between and the
    // wrong one is a tab flying over an open dialog.
    assert!(
        at(19) > at(1),
        "the card that is moving is above the column it came out of"
    );
    assert!(
        at(19) < at(5) && at(19) < at(9) && at(19) < at(11),
        "and below the modal, the tip and the ghost: it is the top of its \
             own list, not the top of the window"
    );
    // P2-9 slice 1: the two instruments that share a terminal pane's right
    // edge. They never overlap a pixel, so this is a statement about which
    // one survives if the lane arithmetic is ever broken.
    assert!(
        at(13) > at(16),
        "the command rail is painted over the scroll thumb beside it"
    );
    assert!(
        at(4) > at(2) && at(4) > at(1),
        "and the float still covers the panes, the rail and the dock drawing"
    );
    // The command marks rail belongs *to* a pane, so every surface that
    // floats over the window is entitled to cover it — a pane menu dropped
    // over a rail must not have ticks showing through it.
    assert!(
        at(13) < at(1) && at(13) < at(4) && at(13) < at(7),
        "a pane's own ticks are under the chrome rail, the float and the pane menu"
    );
    assert!(
        at(9) > at(5) && at(9) > at(4),
        "the tip is covered by nothing"
    );
    // The notices (user ruling, 2026-08-16): over every menu, because a card
    // anchored to the top of a pane's body is exactly where a pane menu drops
    // into — and under the tip, because the `×` on that card has a tip of
    // its own.
    assert!(
        at(8) > at(7) && at(8) > at(6) && at(8) > at(5),
        "a notice is not covered by the menus or the modal family"
    );
    assert!(
        at(8) < at(9),
        "and it does not cover the tip about its own ×"
    );
    // §7.1.5e′ — the card a held modifier raises, between the two. Above the
    // notices because a card the reader summoned stands in front of one the
    // window volunteered, and below the tip on the tip's own standing rule.
    // Its place among the menus and the modal family is bookkeeping and is
    // asserted as *nothing*: the card is never raised while any of them holds
    // the keyboard — see `Runtime::key_hints_offered`.
    assert!(
        at(20) > at(8),
        "the hint card stands over the notices it may share a floor with"
    );
    assert!(
        at(20) < at(9),
        "and under the tip, which is the surface that explains what is under it"
    );
    // The pane head's menu sits on the file menu's level, above every
    // surface either of them can be raised over.
    assert!(
        at(7) > at(5) && at(7) > at(4) && at(7) > at(2),
        "the pane menu is drawn over the panes, the float and the modal family"
    );
    // P143 — `.file-peek { z-index: 70 }` against `#files-flyout`'s 60: the
    // glance card is drawn over the pinned window, because "flyout rows peek
    // too" and a card that its own host covered would be unreadable in the
    // very place the ruling names.
    assert!(
        at(10) > at(4) && at(10) > at(9),
        "the glance card stands over the float it may have been raised from"
    );
}

/// PIN (U12 stage C, the headline): two panes standing in two directories
/// save two directories and come back holding them, uncrossed and
/// uncollapsed.
///
/// The bug this closes is a one-liner with a large blast radius:
/// `Seats::to_persisted` took a single `TermLeafV1` and cloned it into
/// *every* Terminal leaf, so a split tab wrote the first pane's `cwd` twice
/// and both panes came back in the first one's folder. `TermLeafV1` has
/// always been per leaf on disk, so nothing about the schema was wrong — the
/// writer simply had one fact where the file had two slots. Version stays 4
/// and `migrate.rs` is untouched.
///
/// Both halves are pinned here, because a `TabState` needs a `Renderer` and
/// cannot be built in a unit test: the *write* is `to_persisted` against a
/// closure that answers differently per seat, and the *read* is `revive_plan`
/// over that very tree. They meet on the persisted bytes, which is where they
/// meet in the product.
///
/// The pairing rule the read half depends on is stated on [`revive_plan`]:
/// `Seats::from_persisted` mints seat ids by the same in-order walk
/// `seats_in_order` uses, so zipping the persisted tree's Term leaves in
/// order against `Seats::terminals()` pairs each saved folder with the seat
/// that produced it. The `assert_ne!` is what makes a crossed pairing fail
/// rather than merely look odd.
///
/// Two real directories, because the read half filters on `is_dir` — a saved
/// folder that no longer exists is answered the same way as one that was
/// never reported.
#[test]
fn two_panes_save_and_revive_their_own_two_folders() {
    let here = std::env::current_dir().expect("a test runs somewhere");
    let up = here
        .parent()
        .expect("a crate directory has a workspace above it")
        .to_path_buf();
    assert_ne!(here, up, "the two halves need two distinct real folders");

    // ── the write half ──
    let seats = seats::Seats::from_persisted(&saved_row_of_two("", "").root);
    let [left, right] = seats.terminals()[..] else {
        panic!("a row of two terminals holds two terminal seats");
    };
    // Answered by seat id, and refusing any other — which also pins that
    // the closure is asked about each terminal leaf exactly once, by name.
    let saved = seats.to_persisted(
        &|seat| TermLeafV1 {
            profile_id: "pwsh".to_owned(),
            cwd: match seat {
                seat if seat == left => here.to_string_lossy().into_owned(),
                seat if seat == right => up.to_string_lossy().into_owned(),
                other => panic!("only the tree's own terminal seats are asked, not {other:?}"),
            },
            manual_name: None,
            card_skip: 0,
            last_command: String::new(),
        },
        &|seat| panic!("a tree of two terminals has no files leaf to ask about ({seat:?})"),
    );
    let written = persisted_term_leaves(&saved);
    assert_eq!(written.len(), 2, "two leaves, asked about separately");
    assert_eq!(written[0].cwd, here.to_string_lossy());
    assert_eq!(
        written[1].cwd,
        up.to_string_lossy(),
        "the second leaf is asked about itself, not handed the first's answer"
    );

    // ── the read half ──
    let (revived, _, folders, _files, _preview) = revive_plan(&TabV1 {
        root: saved,
        pinned: false,
        focused_leaf: "leaf-0".to_owned(),
        preview: None,
    });
    let [revived_left, revived_right] = revived.terminals()[..] else {
        panic!("the tree came back with its two terminals");
    };
    let folder = |seat: SeatId| {
        folders
            .get(&seat)
            .and_then(|leaf| leaf.cwd.as_ref().map(|place| place.path().to_path_buf()))
    };
    assert_eq!(folders.len(), 2, "not collapsed to one");
    assert_eq!(
        folder(revived_left),
        Some(here),
        "the first leaf's folder lands on the first seat"
    );
    assert_eq!(
        folder(revived_right),
        Some(up),
        "and the second's on the second — not crossed"
    );
    assert_ne!(
        folder(revived_left),
        folder(revived_right),
        "two panes, two places"
    );
}

/// PIN (C28, by its letter): a terminal pane names itself with its own
/// program's title, else its own folder **whole**, else nothing.
///
/// C28 writes `${s.cwd}` into `.ptitle` (mock-up 4559) — the entire path —
/// and it does so in deliberate contrast with the drag ghost and the drop
/// preview, which write `cwdLeaf(s)` (mock-up 3304). An earlier stage of this
/// slice recorded a user ruling narrowing the head to the leaf as well; the
/// user has since overturned that as a typo in their own work order, so the
/// inventory's letter is authoritative again and the contrast is real rather
/// than degenerate. The second assertion is the one that states it: a head
/// has a whole bar to fill and answers "where is this" in full, while a label
/// riding the pointer has one line and answers "which one is this".
///
/// The `NameSource` handed back with the path is still `Cwd` and not some
/// third provenance, because the layer that won is the same layer: the tab's
/// tooltip reports *which* layer spoke, never how long the answer it gave
/// was. Only the rendering of the folder differs between the two readers, and
/// that is exactly the one thing [`session_title`] takes as a parameter.
///
/// The tab's `manual_name` is deliberately not in this stack: it is a
/// tab-level override and printing it on every pane would be one name for
/// several rooms, which is the whole bug. That is pinned by the last
/// assertion, where the same folder resolves the same way whatever the tab
/// is called — `session_title` cannot see the tab at all.
///
/// Red gate: point the head's place layer back at [`cwd_leaf`] instead of
/// [`cwd_whole`] and the whole-path assertions fail; drop the sanitiser's
/// fall-through and the control-character case names the pane after a
/// program that said nothing; **drop the profile-title filter — treat the
/// profile's own name as a real announcement — and the first assertion prints
/// `PowerShell · D:\…` on every head in the window, which is the bug.**
#[test]
fn a_pane_head_says_where_it_is_and_a_program_speaks_only_in_front_of_it() {
    let cwd = Path::new(r"D:\Developer\folio-terminal\crates\bt-app");
    let whole = r"D:\Developer\folio-terminal\crates\bt-app".to_owned();
    let profile = profiles::title(profiles::fallback_profile());

    // THE BUG. `scripts/shell-integration/folio.ps1` ends by
    // writing `ESC ]0;PowerShell BEL`, so every session in this window
    // carries this exact title for its whole life. Under the old order it
    // outranked the folder, and a four-pane split printed one word four
    // times while the location — the only thing a head is for — was gone
    // from all four.
    assert_eq!(
        pane_head_title(Some(profile), Some(cwd), &[profile]),
        Some((whole.clone(), tooltip::NameSource::Cwd)),
        "a shell that only agrees with its profile has announced nothing"
    );
    // A program with something of its own to say rides in FRONT of the
    // place, and does not replace it: a head has a whole bar and can carry
    // both facts at once.
    assert_eq!(
        pane_head_title(Some("vim main.rs"), Some(cwd), &[profile]),
        Some((
            format!("vim main.rs{}{whole}", tooltip::NAME_PLACE_SEPARATOR),
            tooltip::NameSource::Program
        )),
        "`title · path`, the tip's own punctuation"
    );
    // The suppression is measured against THIS profile's name, not against
    // a literal. Hard-code `\"PowerShell\"` here and the day a second
    // profile ships, its panes start repeating a word already on the tab —
    // and this assertion is what notices.
    assert_eq!(
        pane_head_title(Some("PowerShell"), Some(cwd), &["Ubuntu"]),
        Some((
            format!("PowerShell{}{whole}", tooltip::NAME_PLACE_SEPARATOR),
            tooltip::NameSource::Program
        )),
        "under another profile that same word IS an announcement"
    );
    // Measured on the sanitised title, like every other decision here, so a
    // control character glued to the profile's name cannot smuggle the
    // prefix back in.
    let smuggled = profile.replacen(' ', "\u{1b} \u{8}", 1) + "\u{7f}";
    assert_eq!(
        pane_head_title(Some(&smuggled), Some(cwd), &[profile]),
        Some((whole.clone(), tooltip::NameSource::Cwd)),
        "the check sees what the head would print, not what arrived"
    );

    // With no title at all it is the WHOLE folder, not its last segment —
    // C28's own `${s.cwd}`, the answer to "where is this".
    assert_eq!(
        pane_head_title(None, Some(cwd), &[profile]),
        Some((whole.clone(), tooltip::NameSource::Cwd)),
        "the whole path; the leaf is what a label riding the pointer says"
    );
    // A title that sanitises away has said nothing, and falls through rather
    // than blanking the head — the impersonation the sanitiser refuses.
    assert_eq!(
        pane_head_title(Some("\u{1b}\u{7}\u{8}"), Some(cwd), &[profile]),
        Some((whole.clone(), tooltip::NameSource::Cwd)),
        "an empty sanitised layer is not an answer"
    );
    assert_eq!(
        pane_head_title(Some("   "), Some(cwd), &[profile]),
        Some((whole.clone(), tooltip::NameSource::Cwd))
    );

    // NO FOLDER: the chain the head has always had, and the profile filter
    // is deliberately absent from it. Suppressing the title here would
    // leave the head emptier than the shell left it, because there is no
    // better answer underneath to reveal.
    assert_eq!(
        pane_head_title(Some(profile), None, &[profile]),
        Some((profile.to_owned(), tooltip::NameSource::Program)),
        "with nowhere to be, the profile's own word is still the best there is"
    );
    assert_eq!(
        pane_head_title(Some("vim main.rs"), None, &[profile]),
        Some(("vim main.rs".to_owned(), tooltip::NameSource::Program))
    );
    // The path layer is sanitised on the same terms, because OSC 7 is as
    // program-controlled as OSC 2: a folder that sanitises to nothing has
    // named no place, and falls through exactly as an empty title does.
    assert_eq!(
        pane_head_title(None, Some(Path::new("\u{1b}\u{7}")), &[profile]),
        None,
        "a path that sanitises away is not a place either"
    );
    // Neither layer: nobody has said anything, and the caption falls back to
    // the seat's kind where it is drawn rather than to a guess here.
    assert_eq!(pane_head_title(None, None, &[profile]), None);
    assert_eq!(pane_head_title(Some(""), None, &[profile]), None);

    // Two panes, two answers — the whole point of resolving per leaf. They
    // differ in the last segment, and at the head's length they carry the
    // shared prefix that says which tree the two of them are in. This is the
    // pair that used to be two identical "PowerShell"s.
    assert_ne!(
        pane_head_title(
            Some(profile),
            Some(Path::new(r"C:\repo\crates\bt-app")),
            &[profile]
        ),
        pane_head_title(
            Some(profile),
            Some(Path::new(r"C:\repo\crates\bt-term")),
            &[profile]
        ),
    );

    // And the tab's own name is not in this stack at all: `resolve_title`
    // adds it on top for the *tab*, and a pane never sees it.
    assert_eq!(
        resolve_title(
            Some("my tab"),
            None,
            Some(cwd),
            profiles::title(profiles::fallback_profile()),
            &[profile],
        )
        .0,
        "my tab",
        "the tab wears the override"
    );
    // **The filter, though, is now one ruling with two readers** (user
    // ruling 2026-09-07). This assertion used to read `profile` and said
    // "the tab takes a title over a folder, filter or no filter" — which is
    // how a PowerShell tab came to sit on `PowerShell 7` while the head over
    // its own pane said `D:\Demo`. What separates the two stacks is the
    // *rendering* of the folder and the manual layer above it, not whether a
    // shell agreeing with its launcher has spoken.
    assert_eq!(
        resolve_title(
            None,
            Some(profile),
            Some(cwd),
            profiles::title(profiles::fallback_profile()),
            &[profile],
        )
        .0,
        cwd_leaf(cwd).expect("the folder this test stands in"),
        "the tab takes the folder's leaf where the head takes the whole path"
    );
    assert_eq!(
        pane_head_title(None, Some(cwd), &[profile]).map(|(name, _)| name),
        Some(whole),
        "the pane under it does not"
    );
}

/// PIN: the head's folder is bounded by [`CWD_MAX_CHARS`] and by nothing
/// shorter.
///
/// Two assertions and they are opposites, because a cap has two ways to be
/// wrong and only both of them together say which one this is. The first is
/// the failure the reversal exists to prevent: an ordinary working directory
/// — longer than [`TITLE_MAX_CHARS`], as nearly every real one is — must
/// arrive at the head with every character it had. Cap the path layer at the
/// name layer's forty and this is what breaks, silently, while a head that
/// merely *looks* full keeps drawing.
///
/// The second is the bound itself. OSC 7 is program-controlled, so "print it
/// whole" cannot mean "print whatever arrives": an unbounded head shapes
/// whatever length a program chose, on every frame it is drawn. The path
/// built here is a legal one — real separators, real segments — rather than
/// a run of one character, because a bound that only holds against obvious
/// junk is not holding against the input that would actually arrive.
///
/// Red gate: hand the path layer `TITLE_MAX_CHARS` and the first assertion
/// fails at forty; drop the maximum entirely and the second stops bounding
/// anything.
#[test]
fn a_pane_heads_folder_is_capped_at_max_path_and_not_at_a_names_forty() {
    let profile = profiles::title(profiles::fallback_profile());
    let ordinary = Path::new(r"D:\Developer\folio-terminal\crates\bt-app\src");
    let ordinary_text = ordinary.to_str().expect("a test path is UTF-8");
    assert!(
        ordinary_text.chars().count() > TITLE_MAX_CHARS,
        "the fixture only proves anything if a name's cap would have bitten"
    );
    assert_eq!(
        pane_head_title(None, Some(ordinary), &[profile]),
        Some((ordinary_text.to_owned(), tooltip::NameSource::Cwd)),
        "an ordinary path is not cut at all — C28's whole path, whole"
    );

    // A path no filesystem can hold, which is what a hostile or broken OSC 7
    // looks like. It is cut to the bound and the head's shaping cost with
    // it.
    let segment = "directory";
    let long = format!(
        r"D:\{}",
        std::iter::repeat_n(segment, 200)
            .collect::<Vec<_>>()
            .join("\\")
    );
    assert!(long.chars().count() > CWD_MAX_CHARS);
    let (named, source) = pane_head_title(None, Some(Path::new(&long)), &[profile])
        .expect("a long path is still a place");
    assert_eq!(
        named.chars().count(),
        CWD_MAX_CHARS,
        "`MAX_PATH` characters, and the next one is not a folder"
    );
    assert_eq!(source, tooltip::NameSource::Cwd);
    assert!(
        long.starts_with(&named),
        "cut from the end, so what survives is the path it really was"
    );

    // And the two bounds stay two: a *name* is still capped where a name has
    // always been capped, so widening the path layer did not widen the tab.
    assert_eq!(
        display_title(
            None,
            Some(&"x".repeat(CWD_MAX_CHARS)),
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        )
        .chars()
        .count(),
        TITLE_MAX_CHARS,
        "a program title is a name, and forty characters is a name's length"
    );
}

/// One OSC 7 report, the way a shell writes it.
///
/// Built rather than borrowed, because there is no way to hand a
/// [`DualPlaneSession`] a working directory except the way a shell does: the
/// field is private to `bt-term` and is only ever written by the parser
/// acting on a `file://` URI. That is a feature for this pin — it means the
/// path under test travels the whole road a real one travels, escape
/// sequence and percent-decoding included, rather than being posted into the
/// middle of it.
fn osc7_report(path: &str) -> String {
    format!("\u{1b}]7;file:///{}\u{7}", path.replace('\\', "/"))
}

/// The title `scripts/shell-integration/folio.ps1` writes, in its
/// own bytes.
///
/// Copied from the script's last line rather than posted straight into
/// `window_title`, for the same reason [`osc7_report`] is built: the string
/// under test has to travel the road a real one travels. It is the shipped
/// integration's actual parting shot, and it is the input that made this
/// ruling necessary.
fn osc0_report(title: &str) -> String {
    format!("\u{1b}]0;{title}\u{7}")
}

/// PIN (C28, end to end): the names a *tab* hands the chrome are the whole
/// folders its shells reported.
///
/// The other C28 pins in this module test the walk; this one tests the
/// **wiring**, and the difference is the whole reason it exists. Nothing
/// above it can tell [`TabState::terminal_name`] from a version of itself
/// that asks for the tab's rendering instead of the head's — every one of
/// them names [`pane_head_title`] directly, so all of them stay green while
/// the window prints leaves. This one goes through the seat map a real frame
/// reads, from two sessions that were told where they stand the only way a
/// session can be told: an OSC 7 escape, parsed.
///
/// Two panes under one repository, so the assertion cannot pass on the
/// strength of two unlike words. They agree on every character but the last
/// segment, which is precisely the case the reversed ruling argued the leaf
/// was needed for — and C28's answer is that the head has the room, so it
/// says the whole thing and the *label* does the shortening (C29).
///
/// Red gate: point [`pane_head_title`] at [`CWD_AS_LEAF`], or have
/// `terminal_name` resolve through the tab's stack, and both entries collapse
/// to their last segment.
#[test]
fn a_tabs_terminal_names_are_the_whole_folders_its_shells_reported() {
    let left_path = r"D:\Developer\folio-terminal\crates\bt-app";
    let right_path = r"D:\Developer\folio-terminal\crates\bt-term";
    let profile = profiles::title(profiles::fallback_profile());
    // Both shells say exactly what the shipped integration makes them say:
    // the profile's own title, then where they stand. This is the pane pair
    // that used to read "PowerShell" twice.
    let tab = cross_tab(
        1,
        &[
            &format!("{}{}", osc0_report(profile), osc7_report(left_path)),
            &format!("{}{}", osc0_report(profile), osc7_report(right_path)),
        ],
    );
    let [left, right] = tab.seats.terminals()[..] else {
        panic!("a two-pane cross tab holds two terminal seats");
    };
    assert_eq!(
        tab.sessions[&left].session.window_title(),
        Some(profile),
        "the fixture only proves anything if the integration's title really \
             arrived and really outranked the folder under the old order"
    );

    let names = tab.terminal_names();
    assert_eq!(
        names.get(&left).map(String::as_str),
        Some(left_path),
        "the head over a shell standing here says where it stands, whole"
    );
    assert_eq!(names.get(&right).map(String::as_str), Some(right_path));
    assert_eq!(
        tab.terminal_name(left).as_deref(),
        Some(left_path),
        "and the per-seat lookup agrees with the map built out of it"
    );

    // The contrast C28 is a ruling about, at the far end of the wiring: the
    // chrome is handed these names and cuts its own short ones from them.
    assert_eq!(
        seats::seat_short_caption(
            bt_layout::SeatKind::Terminal,
            None,
            names.get(&left).map(String::as_str),
            None,
        ),
        "bt-app",
        "3304: the label riding the pointer still gets the one word"
    );
}

/// One turn, with the window's three facts spelled out and the deliveries handed back.
fn one_turn_reaching(
    tabs: &mut [TabState],
    active: usize,
    focused: bool,
    hidden: bool,
    turn_end_enabled: bool,
    next: &mut attention::Places,
) -> Vec<AttentionDelivery> {
    let mut raised = Vec::new();
    settle_attention(
        tabs,
        active,
        notify::WindowPlace {
            hidden,
            ..on_a_screen(focused)
        },
        attention::NotificationSwitches {
            turn_end: turn_end_enabled,
            desktop_messages: true,
        },
        next,
        Instant::now(),
        None,
        &mut raised,
    );
    raised
}

/// PIN (`attention` plan §10.7's three tiers and §11.7's switch, gate ⑯) — **the end of a
/// turn reaches exactly as far as the reader is away, and the row switches the whole lane
/// off.**
///
/// The three cells the ruling names, walked through the window's own pass rather than through
/// the ledger directly, because the thing being pinned is that the *facts of the window* reach
/// the ledger's door: which tab is on screen, whether the window has the keyboard, and whether
/// it is on a screen at all. Before slice C the third of those did not exist and the pass
/// answered the weaker of two tiers for every window in the world.
///
/// MUTATIONS: drop `window_is_hidden` from the pass and the minimised cell answers `Flash` —
/// a reader whose window is not on any screen gets a taskbar flash they cannot see, and never
/// the toast. Pass `true` for the switch unconditionally and the last block goes red in both
/// halves: the lane runs, and the bit it sets means turning the row back on mid-turn finds the
/// turn already dealt with.
#[test]
fn a_turn_ending_reaches_as_far_as_the_reader_is_away_and_the_row_shuts_the_lane() {
    let cells = [
        (true, true, false, attention::Reach::Nothing),
        (false, true, false, attention::Reach::Flash),
        (true, false, false, attention::Reach::Flash),
        (true, false, true, attention::Reach::Toast),
        (false, true, true, attention::Reach::Toast),
    ];
    for (active, focused, hidden, expected) in cells {
        let mut tabs = vec![ringing_tab(1, 1), ringing_tab(2, 1)];
        let index = usize::from(!active);
        let seat = tabs[0].seats.terminals()[0];
        ring(&mut tabs[0], seat);
        let mut next = attention::Places::default();
        let raised = one_turn_reaching(&mut tabs, index, focused, hidden, true, &mut next);
        assert_eq!(
            raised.len(),
            1,
            "one turn ended and one interruption was decided \
                 (active={active} focused={focused} hidden={hidden})"
        );
        assert_eq!(raised[0].why, attention::Why::TurnEnd);
        assert_eq!(
            raised[0].reach, expected,
            "active={active} focused={focused} hidden={hidden}"
        );
        assert_eq!(
            next.issued(),
            0,
            "and not one place was handed out: a turn ending is not a request (red line 14)"
        );
    }

    // The row, off: the lane is not walked, so nothing is decided and nothing is remembered.
    let mut tabs = vec![ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    ring(&mut tabs[0], seat);
    let mut next = attention::Places::default();
    assert!(
        one_turn_reaching(&mut tabs, 0, false, true, false, &mut next).is_empty(),
        "with the row off a minimised window hears nothing"
    );
    // And the bit was not set behind the shut door, so the very next turn of the pass with the
    // row back on still has a turn end to decide about.
    tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat")
        .bell_reported = false;
    assert_eq!(
        one_turn_reaching(&mut tabs, 0, false, true, true, &mut next).len(),
        1,
        "a row turned off and on again left no half state behind"
    );
}

/// PIN — **`BT_ATTENTION_TRACE` says which of the queue's decisions happened, and says each of
/// them once** (`attention` plan §11.1.5's field contract).
///
/// The whole reason this trace exists: the orange ring is the only thing the queue ever says out
/// loud, and it says the same word for every reason it could be lit. A user reporting "it never
/// goes out" was reporting a result; these lines are the decisions behind it.
///
/// Three properties, and the last two are the ones a station is likely to lose: every decision
/// writes a line; **nothing that decided nothing writes one** (the pass runs on every turn of
/// the event loop, so a station that spoke whenever it merely *saw* a level would drown the
/// file); and **every line names the request it is about**, `episode=-` included.
#[test]
fn the_attention_trace_writes_one_line_per_decision_and_none_otherwise() {
    let path = std::env::temp_dir().join(format!(
        "{}.log",
        bt_testpath::unique_name("bt-attention-trace-stations")
    ));
    let _ = std::fs::remove_file(&path);
    let trace = crate::trace::Trace::create(&path, "# pin");
    let read = |from: usize| -> Vec<String> {
        std::fs::read_to_string(&path)
            .expect("the trace file was created")
            .lines()
            .skip(1 + from)
            .map(|line| {
                line.trim_start()
                    .split_once(' ')
                    .expect("a timestamp leads every line")
                    .1
                    .to_owned()
            })
            .collect()
    };
    let turn = |tabs: &mut [TabState], active: usize, next: &mut attention::Places| {
        settle_attention(
            tabs,
            active,
            on_a_screen(true),
            BOTH_NOTIFICATION_ROWS_ON,
            next,
            Instant::now(),
            Some(&trace),
            &mut Vec::new(),
        );
    };

    let mut tabs = vec![ringing_tab(1, 1), ringing_tab(2, 2)];
    let watched = tabs[0].seats.terminals()[0];
    let background = tabs[1].seats.terminals()[1];
    let mut next = attention::Places::default();
    let mut written = 0;
    let next_lines = |written: &mut usize, expected: &[String]| {
        let lines = read(*written);
        assert_eq!(lines, expected, "written from line {written}");
        *written += lines.len();
    };

    // A quiet turn decides nothing, and says nothing.
    turn(&mut tabs, 0, &mut next);
    next_lines(&mut written, &[]);

    // **A bell where the user is looking.** It is a turn ending, so it is decided about once and
    // reaches nothing; it takes no place, and the only other line is the dot the look put out.
    ring(&mut tabs[0], watched);
    turn(&mut tabs, 0, &mut next);
    next_lines(
        &mut written,
        &[
            format!(
                "toast tab=0 seat={watched:?} why=turn-end episode=- reach=nothing src=bel \
                     via=bel"
            ),
            "claim tab=0 episode=- was=Bell now=Silent".to_owned(),
        ],
    );

    // **A standing request behind a closed lid**: minted, admitted, and the tab's dot changed.
    // No `toast`, because a program that says it wants you has not said it is blocked on you.
    request_attention(&mut tabs[1], background, "yes");
    turn(&mut tabs, 0, &mut next);
    next_lines(
        &mut written,
        &[
            format!(
                "mint tab=1 seat={background:?} episode=1 src=osc gen=1 grounds=requested prev=-"
            ),
            format!(
                "admit tab=1 seat={background:?} ticket=0 episode=1 grounds=requested \
                     active=0 focused=1"
            ),
            "claim tab=1 episode=1 was=Silent now=Awaiting".to_owned(),
        ],
    );

    // Restating it while it stands decides nothing — and this is the line that would repeat
    // sixty times a second if it did.
    for _ in 0..3 {
        request_attention(&mut tabs[1], background, "yes");
        turn(&mut tabs, 0, &mut next);
    }
    next_lines(&mut written, &[]);

    // Looking at it keeps the place, and says nothing at all about it: the ledger holds no
    // latch for a look to spend.
    turn(&mut tabs, 1, &mut next);
    next_lines(&mut written, &[]);

    // Answering is the door out, and it names what it answered. **No `claim` line follows**,
    // and that is the station's own rule rather than an omission: a `claim` reports what the
    // *pass* did to the dot, and the dot went out at the keystroke — which the `answer` line
    // above is the record of.
    answer(
        &mut tabs,
        1,
        background,
        UserInputKind::Keyboard,
        &mut next,
        Some(&trace),
    );
    turn(&mut tabs, 1, &mut next);
    next_lines(
        &mut written,
        &[format!(
            "answer tab=1 seat={background:?} ticket=0 episode=1 by=keyboard weak=1 strong=0"
        )],
    );

    // And the program withdrawing what it asked closes the episode.
    request_attention(&mut tabs[1], background, "no");
    turn(&mut tabs, 1, &mut next);
    next_lines(
        &mut written,
        &[
            format!("clear tab=1 seat={background:?} episode=1 src=osc gen=1 reason=program"),
            format!("drop tab=1 seat={background:?} episode=1 reason=program"),
        ],
    );

    // **Every line names its request**, and the two spellings of "none" are the honest ones.
    let all = read(0);
    assert!(!all.is_empty());
    for line in &all {
        assert!(
            line.contains(" episode="),
            "a line that cannot say which request it is about: {line}"
        );
    }
    assert!(
        !all.iter().any(|line| line.contains("by=mouse-motion")),
        "a pointer sweep can never reach the answer door"
    );

    let _ = std::fs::remove_file(&path);
}

/// RED (review row R4-1) — **a tab whose program is gone comes back as a
/// pane, not as a panic.**
///
/// `create_leaf_session` took the resolved program with an `expect`, and the
/// invariant behind it was the picker's greying — which is true of the picker
/// and was never true of the startup restore. `revive_plan` reads a profile
/// id out of `session.json`; its own degradation covers an id this build does
/// not *recognise* and says nothing about one it recognises perfectly well
/// whose program is no longer installed. Uninstall Git with a Git Bash tab
/// pinned, and the next launch panicked before the first window — every
/// launch, until the session file was edited by hand.
///
/// The three answers below are the whole rule, and the third one matters as
/// much as the second: falling back is only honest while there is something
/// to fall back to, and a machine with nothing at all gets a pane that says
/// so rather than a pane pretending to be a shell.
///
/// Red gate: `panic!` when `programs.program(..)` is `None` — which is what
/// this replaced — and the second and third cases here are the panic.
#[test]
fn a_profile_this_machine_cannot_start_falls_back_instead_of_panicking() {
    let git = profiles::index_of_id("gitbash");
    let fallback = profiles::fallback_profile();
    let fallback_id = profiles::fallback_profile_id();
    assert_ne!(git, fallback, "the fixture needs two different rows");

    let equipped = profiles::ProfilePrograms::with_only(&[git, fallback]);
    assert_eq!(
        startable_profile("gitbash", &equipped),
        Ok(Started::AsAsked)
    );

    // Git uninstalled between two launches, which is the row's own case.
    let gitless = profiles::ProfilePrograms::with_only(&[fallback]);
    assert_eq!(
        startable_profile("gitbash", &gitless),
        Ok(Started::FellBack(fallback_id.to_owned())),
        "the pane comes back running what this machine does have"
    );
    assert_eq!(
        startable_profile(fallback_id, &gitless),
        Ok(Started::AsAsked),
        "and the profile standing in for the others is not standing in for itself"
    );

    // Nothing at all: a machine with no Windows PowerShell, or a `BT_SHELL`
    // pointed at a program that is not there.
    let bare = profiles::ProfilePrograms::with_only(&[]);
    assert_eq!(startable_profile("gitbash", &bare), Ok(Started::Nothing));
    assert_eq!(startable_profile(fallback_id, &bare), Ok(Started::Nothing));

    // **An id the table does not hold at all** — a row somebody deleted in
    // Settings ▸ Profiles while a pane was running it, which the snapshot
    // above still has a program for. The live table is asked first, so this
    // is the fall and not the old out-of-bounds read.
    assert!(!profiles::has_id("a-row-nobody-has"));
    assert_eq!(
        startable_profile("a-row-nobody-has", &equipped),
        Ok(Started::FellBack(fallback_id.to_owned())),
        "a profile that is gone degrades exactly as a missing program does"
    );
}

/// **Same rhythm, not merely eventual.** The turn count from "the shell
/// spoke" to "it is on the glass" is measured for each pane in turn, under
/// the identical script. Focus is the only difference between the two, and
/// it must make no difference to this number.
#[test]
fn both_panes_reach_the_glass_in_the_same_number_of_turns() {
    let turns_until_shown = |sibling_speaks: bool| {
        let mut harness = TwoPaneHarness::new(24, 6);
        harness.turn(b"prompt\r\n", b"prompt\r\n", pty_drain_says_nothing_new);
        let (focused_bytes, sibling_bytes): (&[u8], &[u8]) = if sibling_speaks {
            (b"", b"needle\r\n")
        } else {
            (b"needle\r\n", b"")
        };
        let mut spoken = false;
        for turn in 1..=10 {
            harness.turn(
                if spoken { b"" } else { focused_bytes },
                if spoken { b"" } else { sibling_bytes },
                pty_drain_says_nothing_new,
            );
            spoken = true;
            let shown = if sibling_speaks {
                harness.sibling_shows("needle")
            } else {
                harness.focused_shows("needle")
            };
            if shown {
                return turn;
            }
        }
        u32::MAX
    };

    assert_eq!(turns_until_shown(false), 1, "the pane with the keyboard");
    assert_eq!(
        turns_until_shown(true),
        turns_until_shown(false),
        "and the pane without it, on the same turn"
    );
}

/// PIN — C25: a tab's name is the topmost layer with something to say.
/// 手动 > 程序标题 (OSC 2) > cwd 叶名 (OSC 7) > profile (mock-up line 2593).
///
/// Red gate: the name used to be `window_title().unwrap_or(profile)`, which
/// had no manual layer at all and fell from OSC 2 straight past the shell's
/// own report to the profile.
#[test]
fn a_tab_name_takes_the_most_specific_layer_that_actually_spoke() {
    let cwd = Path::new(r"D:\Developer\folio-terminal");
    assert_eq!(
        display_title(
            Some("我的构建"),
            Some("pwsh"),
            Some(cwd),
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        "我的构建",
        "what you typed outranks everything under it"
    );
    assert_eq!(
        display_title(
            None,
            Some("Claude ✳ 任务"),
            Some(cwd),
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        "Claude ✳ 任务",
        "then what the program announced"
    );
    assert_eq!(
        display_title(
            None,
            None,
            Some(cwd),
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        "folio-terminal",
        "then where the shell says it is standing"
    );
    assert_eq!(
        display_title(
            None,
            None,
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        profiles::title(profiles::fallback_profile()),
        "and the profile catches what is left"
    );
}

/// PIN — **a tab's name follows the folder its pane is standing in, for
/// every shell** (user ruling 2026-09-07; ticket T-1 of
/// `docs/plans/shell-matrix-2026-09-07.md`).
///
/// Red gate, and it is the measured defect: `folio.ps1` ends every prompt by
/// writing `ESC ]0;PowerShell 7 BEL`, deliberately the very name the profile
/// already goes by, and the program layer used to admit any title at all. So
/// every PowerShell tab in the product was called `PowerShell 7` for the
/// life of the pane while the pane stood in `D:\Demo` — the folder layer
/// beneath it was unreachable. With [`announced_layer`] gone the assertions
/// below read `PowerShell 7`, `Windows PowerShell 5.1` and `Command Prompt`
/// where they now read a folder.
///
/// The other three rules are here because this ruling is the one place they
/// could be broken while looking fixed: a tab you named keeps your name, a
/// shell that says something of its own is shown saying it, and the profile
/// is what is left when no folder is known.
#[test]
fn a_tab_follows_its_folder_when_the_shell_only_repeats_its_launchers_name() {
    let demo = Path::new(r"D:\Demo");
    let named = |manual: Option<&str>, program: Option<&str>, cwd, id: &str| {
        let profile = profiles::index_of_id(id);
        display_title(
            manual,
            program,
            cwd,
            profiles::title(profile),
            &profiles::announcement_set(profile),
        )
    };

    // ① The defect itself, in all three shells whose integration announces
    // the name their own launcher chose.
    assert_eq!(
        named(None, Some("PowerShell 7"), Some(demo), "pwsh"),
        "Demo",
        "a shell agreeing with its launcher has announced nothing, so the \
             folder names the tab"
    );
    assert_eq!(
        named(None, Some("Windows PowerShell 5.1"), Some(demo), "winps"),
        "Demo"
    );
    assert_eq!(
        named(None, Some("Command Prompt"), Some(demo), "cmd"),
        "Demo"
    );
    // And the profile's *shipped* name is refused as well as its displayed
    // one, which is what `announcement_set` exists to say: `WSL · Ubuntu`
    // and the bare `WSL` are one name in two spellings.
    assert_eq!(named(None, Some("WSL"), Some(demo), "wsl"), "Demo");

    // ② A shell that sets a title of its own is shown saying it. Git Bash
    // does, in Git for Windows' own MSYS spelling, and that title is not in
    // its profile's set.
    assert_eq!(
        named(None, Some("MINGW64:/d/Demo"), Some(demo), "gitbash"),
        "MINGW64:/d/Demo",
        "what the shell said outranks where it is standing"
    );
    // Including the one thing a `cmd` pane can say about being busy: the
    // remainder `LeafSession::announced_title` keeps off Windows' console
    // convention is a program announcing what it is running.
    assert_eq!(
        named(None, Some("ping -n 8 127.0.0.1"), Some(demo), "cmd"),
        "ping -n 8 127.0.0.1"
    );

    // ③ A tab you named keeps your name, over both of the layers above.
    assert_eq!(
        named(Some("构建"), Some("PowerShell 7"), Some(demo), "pwsh"),
        "构建"
    );
    assert_eq!(
        named(Some("构建"), Some("MINGW64:/d/Demo"), Some(demo), "gitbash"),
        "构建"
    );

    // ④ And with no folder known at all, the profile is what is left —
    // which is the same string the announcement was, so nothing regresses
    // for a tab whose shell has not yet said where it is.
    let pwsh = profiles::index_of_id("pwsh");
    assert_eq!(
        named(None, Some("PowerShell 7"), None, "pwsh"),
        profiles::title(pwsh)
    );
    assert_eq!(named(None, None, None, "pwsh"), profiles::title(pwsh));

    // The provenance the tip reports moves with the name, because it comes
    // off the same walk: nobody announced anything, so the folder spoke.
    assert_eq!(
        resolve_title(
            None,
            Some("PowerShell 7"),
            Some(demo),
            profiles::title(pwsh),
            &profiles::announcement_set(pwsh),
        ),
        ("Demo".to_owned(), Some(tooltip::NameSource::Cwd))
    );
    // And with no folder under it there is no claim left to report at all.
    assert_eq!(
        resolve_title(
            None,
            Some("PowerShell 7"),
            None,
            profiles::title(pwsh),
            &profiles::announcement_set(pwsh),
        ),
        (profiles::title(pwsh).to_owned(), None)
    );
}

/// PIN — the tab, the pane head and the strip are named by **one** walk, so
/// a PowerShell tab and the head over its own pane cannot disagree about
/// whether that shell has announced anything.
///
/// Red gate: this is T-1's cause stated as a test. The head has filtered the
/// program layer through `profiles::announcement_set` since §7.1.6c-6 and
/// the tab did not, and the gap was invisible as a disagreement — it read as
/// two functions written at different times. Restore the tab's unfiltered
/// layer and the head says `D:\Demo` while the tab above it says
/// `Windows PowerShell 5.1`.
///
/// Through [`TabState`] and not through the two pure functions, because
/// what has to agree is the wiring: the strip and the focus cards are
/// [`TabState::display_title`] and the head is [`TabState::terminal_name`].
#[test]
fn the_tab_and_the_head_over_its_own_pane_agree_about_what_the_shell_said() {
    // The fixture's profile is the fallback one, whose integration
    // announces `Windows PowerShell 5.1`.
    let profile = profiles::fallback_profile();
    let announcement = format!("\u{1b}]0;{}\u{7}", profiles::title(profile));
    let mut leaf = leaf_saying("");
    leaf.session
        .feed(announcement.as_bytes())
        .expect("the shell's own announcement");
    leaf.session
        .feed(b"\x1b]7;file:///D:\\Demo\x1b\\")
        .expect("and the folder it is standing in");
    let tab = tab_holding(leaf);
    let seat = tab.seats.identity();

    assert_eq!(tab.display_title(), "Demo", "the tab has room for one word");
    assert_eq!(
        tab.terminal_name(seat).as_deref(),
        Some(r"D:\Demo"),
        "and the head, which has a whole bar, the whole path — with no \
             launcher's name prefixed to it"
    );
    assert_eq!(
        tab.tooltip_text(),
        "Demo\nWorking folder · D:\\Demo",
        "and the tip names the folder that named the tab"
    );
}

/// PIN — §7.1.4's ladder names a tab too: **before the first report, the
/// folder is where the shell was put down** (user ruling 2026-09-07 — "from
/// `OSC 7`, the integration's cwd report, or the profile's start folder
/// before the first report").
///
/// Red gate: the name stack read `working_directory()`, which is the first
/// rung alone. A tab opened in `D:\Demo` was therefore called
/// `Windows PowerShell 5.1` until its shell finished starting and then
/// jumped — and a pane whose shell never reports `OSC 7` at all never
/// reached the folder layer at any point in its life.
#[test]
fn a_tab_is_named_where_its_shell_was_put_down_until_that_shell_reports() {
    let started_in = PathBuf::from(r"D:\Demo");
    let profile = profiles::fallback_profile();
    let announcement = format!("\u{1b}]0;{}\u{7}", profiles::title(profile));
    let mut leaf = leaf_saying("");
    leaf.session
        .feed(announcement.as_bytes())
        .expect("the shell's own announcement");
    // Both halves of one fact, exactly as the spawn writes them: the field
    // the vault reads and the rung the session's own ladder stands on.
    leaf.spawn_place = Some(started_in.clone());
    leaf.session.set_spawn_directory(Some(started_in.clone()));

    let mut tab = tab_holding(leaf);
    let seat = tab.seats.identity();
    assert_eq!(
        tab.display_title(),
        "Demo",
        "no report yet, and the launcher already knows where it put this shell"
    );
    assert_eq!(tab.terminal_name(seat).as_deref(), Some(r"D:\Demo"));

    // And the first report takes over from it, which is the rung above.
    tab.sessions
        .get_mut(&seat)
        .expect("the fixture's one shell")
        .session
        .feed(b"\x1b]7;file:///D:\\Demo\\notes\x1b\\")
        .expect("the shell says it moved");
    assert_eq!(tab.display_title(), "notes");
}

/// M140: the tab's tip states which layer named it, and the answer comes
/// from the *same* walk that chose the name — including the sanitiser's
/// fall-through, which a second copy of the precedence rules would lose.
#[test]
fn the_tip_names_the_layer_that_actually_named_the_tab() {
    let cwd = Path::new(r"D:\Developer\folio-terminal");
    assert_eq!(
        resolve_title(
            Some("build"),
            Some("pwsh"),
            Some(cwd),
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        )
        .1,
        Some(tooltip::NameSource::Manual)
    );
    assert_eq!(
        resolve_title(
            None,
            Some("pwsh"),
            Some(cwd),
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        )
        .1,
        Some(tooltip::NameSource::Program)
    );
    assert_eq!(
        resolve_title(
            None,
            None,
            Some(cwd),
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        )
        .1,
        Some(tooltip::NameSource::Cwd)
    );
    // The profile's own name is nobody's claim, so there is no provenance to
    // report and the tip says only the name.
    assert_eq!(
        resolve_title(
            None,
            None,
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        )
        .1,
        None
    );
    // A hostile layer that sanitises away does not get the credit for the
    // layer beneath it: it said nothing, so it named nothing.
    assert_eq!(
        resolve_title(
            Some("\u{7}"),
            Some("pwsh"),
            Some(cwd),
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        ("pwsh".to_owned(), Some(tooltip::NameSource::Program))
    );
}

/// The whole second line, assembled — M140's format, F46's extra line, and
/// the full path rather than the leaf the first line already carries.
#[test]
fn a_tabs_tip_is_the_name_then_its_provenance_then_its_promise() {
    let cwd = Path::new(r"D:\Developer\folio-terminal");
    let (name, source) = resolve_title(
        None,
        None,
        Some(cwd),
        profiles::title(profiles::fallback_profile()),
        &profiles::announcement_set(profiles::fallback_profile()),
    );
    let path = cwd.to_string_lossy().into_owned();
    assert_eq!(
        tooltip::tab_tip(&name, source, Some(&path), false),
        format!("folio-terminal\nWorking folder · {path}")
    );
    assert_eq!(
        tooltip::tab_tip(&name, source, Some(&path), true),
        format!("folio-terminal\nWorking folder · {path}\nPinned. Restored next launch")
    );
    // The full path, not the leaf the first line already carries.
    assert!(path.ends_with(r"Developer\folio-terminal"));
}

/// I87: the `+` names the profile it would start, so the button says what it
/// will do rather than merely that it will do something.
///
/// D32/D34: and it names the one it *would* start, not the one it used to.
/// The mock-up folds `state.defaultProfile` into `stripIds()` (4293) purely
/// so this string is repainted when the setting moves; here there is no
/// cached string to repaint, and the pin is that the wording is a function of
/// the argument rather than of the table's first row.
#[test]
fn the_new_tab_button_names_the_profile_it_would_start() {
    assert_eq!(
        new_tab_tip(profiles::fallback_profile_id()),
        "New tab (Windows PowerShell 5.1)"
    );
    assert_eq!(
        new_tab_tip("pwsh"),
        "New tab (PowerShell 7)",
        "the two PowerShells are told apart by the only thing that differs \n             — their version, which is why both titles carry one"
    );
    assert_eq!(
        new_tab_tip("cmd"),
        "New tab (Command Prompt)",
        "point the setting elsewhere and the button says so"
    );
    for index in 0..profiles::count() {
        assert_eq!(
            new_tab_tip(&profiles::id(index)),
            format!("New tab ({})", profiles::title(index))
        );
    }
}

/// PIN — a shell reading its own command line back is not a shell naming
/// itself, so it does not get the layer reserved for shells that do.
///
/// Red gate, and it is a *live* one rather than a hypothetical: `cmd.exe`
/// calls `SetConsoleTitle` with its own image path on the way up, which
/// ConPTY forwards verbatim. Measured through a real pseudoconsole on this
/// machine — `ESC ]0;C:\WINDOWS\System32\cmd.exe BEL`, before the copyright
/// banner. Without this filter every Command Prompt tab in the window is
/// *called* `C:\WINDOWS\System32\cmd.exe`, and stays called that after
/// `OSC 7` finally gives it a real directory, because a program title
/// outranks a folder in the name stack and nothing was ever going to
/// dislodge it.
///
/// The three cases underneath are the ones that must survive: a title the
/// user set, a title that merely *resembles* the program, and a pane with no
/// program at all. A filter that took any of those would be buying cmd's tab
/// name with somebody else's.
#[test]
fn a_title_that_only_repeats_the_program_it_was_started_from_is_not_a_name() {
    let program = PathBuf::from(r"C:\WINDOWS\System32\cmd.exe");
    let titled = |title: &str, program: Option<&Path>| {
        let mut leaf = leaf_saying("x");
        leaf.program = program.map(Path::to_path_buf);
        leaf.session
            .feed(format!("\x1b]0;{title}\x07").as_bytes())
            .unwrap();
        leaf.announced_title().map(str::to_owned)
    };

    assert_eq!(
        titled(r"C:\WINDOWS\System32\cmd.exe", Some(&program)),
        None,
        "cmd's own image path is the launcher's word, not the shell's"
    );
    // Windows' `<image path> - <command line>` convention, which `cmd.exe`
    // uses for as long as a command runs. The launcher's half goes; what
    // the shell actually said stays, and it is the only thing a Command
    // Prompt pane can say about being busy.
    assert_eq!(
        titled(
            r"C:\WINDOWS\System32\cmd.exe - ping  -n 8 127.0.0.1",
            Some(&program)
        )
        .as_deref(),
        Some("ping  -n 8 127.0.0.1")
    );
    // A title that merely shares the prefix is not that convention and is
    // kept whole.
    assert_eq!(
        titled(r"C:\WINDOWS\System32\cmd.exe.log", Some(&program)).as_deref(),
        Some(r"C:\WINDOWS\System32\cmd.exe.log")
    );
    // Windows paths are case-insensitive, and `%SystemRoot%` is spelled
    // `C:\WINDOWS` by the environment and `C:\Windows` by half of everything
    // else. One capital must not restore the defect.
    assert_eq!(titled(r"c:\windows\system32\CMD.EXE", Some(&program)), None);

    // A title the user set in that same pane — `title Build` — is a shell
    // saying something, and outranks the folder exactly as it always did.
    assert_eq!(
        titled("Build", Some(&program)).as_deref(),
        Some("Build"),
        "the filter refuses one string, not the layer"
    );
    // Only the whole path is the command line read back. A program that
    // announces itself as `cmd.exe` has chosen a name.
    assert_eq!(
        titled("cmd.exe", Some(&program)).as_deref(),
        Some("cmd.exe")
    );
    // And a pane with no process behind it has nothing to be repeating.
    assert_eq!(
        titled(r"C:\WINDOWS\System32\cmd.exe", None).as_deref(),
        Some(r"C:\WINDOWS\System32\cmd.exe")
    );

    // The consequence, through the two stacks that read it: with the title
    // refused, the folder is what names both.
    let mut leaf = leaf_saying("x");
    leaf.program = Some(program.clone());
    leaf.session
        .feed(b"\x1b]0;C:\\WINDOWS\\System32\\cmd.exe\x07\x1b]7;file:///D:\\src\x1b\\")
        .unwrap();
    assert_eq!(
        display_title(
            None,
            leaf.announced_title(),
            leaf.standing_in(),
            profiles::title(profiles::index_of_id("cmd")),
            &profiles::announcement_set(profiles::index_of_id("cmd")),
        ),
        "src",
        "the tab wears the folder's leaf"
    );
    assert_eq!(
        pane_head_title(
            leaf.announced_title(),
            leaf.standing_in(),
            &[profiles::title(profiles::index_of_id("cmd"))],
        )
        .map(|(name, _)| name),
        Some(r"D:\src".to_owned()),
        "and the head the whole path, with no executable prefixed to it"
    );
}

/// PIN — the *other* half of "never silently", which was silent.
///
/// `M2-restart-shell-contract.md` §3 names two ways into the fallback and
/// gives them one rule: an unknown `profile_id` and a shell that is gone
/// both fall to the default profile and both must say so. Only the second
/// was implemented. The first is the one that happens without anything
/// breaking — a profile removed from a build, or a session file written by a
/// newer one — and `index_of_id` folded it into `fallback_profile()` before
/// the spawn, so nothing downstream could tell the substitution from an
/// ordinary PowerShell tab.
///
/// Red gate: [`profiles::index_of_id`] alone cannot fail this test, because
/// it answers `fallback_profile()` for a saved `"pwsh"` and a saved `"fish"`
/// alike; it takes [`profiles::has_id`] to know which happened, which is why
/// that function exists.
#[test]
fn a_pane_saved_as_a_profile_this_build_lacks_says_so_rather_than_pretending() {
    assert!(profiles::has_id("cmd") && !profiles::has_id("fish"));
    let banner = unknown_profile_banner("wsl-ubuntu");
    let mut session = DualPlaneSession::with_quotas_and_cell_height(
        nonzero_u32(120),
        nonzero_u32(6),
        DEFAULT_STAGING_QUOTA,
        DEFAULT_FROZEN_LINE_QUOTA,
        std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
    );
    session.feed(banner.as_bytes()).unwrap();
    let first = session.terminal().visible_text()[0].clone();
    assert!(
        first.contains("[Folio]") && first.contains("\"wsl-ubuntu\""),
        "the line names the terminal and quotes the id that is missing: {first:?}"
    );
    assert!(
        first.contains("PowerShell"),
        "and the profile that stood in for it, on the same row: {first:?}"
    );
    assert!(
        session.terminal().visible_text()[1].trim().is_empty(),
        "one sentence, ending its own line so the row beneath belongs to \
             the shell"
    );
    // Same register as its sibling: dim, prefixed, and ending its own line
    // so the shell's first prompt does not print over it.
    assert!(banner.starts_with("\x1b[2m[Folio] ") && banner.ends_with("\x1b[0m\r\n"));
}

/// PIN — a tab opened by `New terminal in folder…` stands in the folder that
/// was chosen, whatever the pane under the menu was standing in (user ruling
/// 2026-08-20).
///
/// Red gate: `new_tab_with_profile` had one answer to "where does this
/// start" — `cwd_for_spawn` off the focused pane — and a row that opens a
/// dialog has a different one. Feeding the chosen folder through the
/// inheritance instead of over it is the failure that looks like the feature
/// working: on a machine whose panes all speak Windows the two answers agree,
/// and the day one of them is a WSL pane the tab opens somewhere else.
///
/// And the cancel: a chooser that comes back with nothing asks for nothing.
#[test]
fn a_tab_opened_in_a_chosen_folder_stands_there_and_not_where_the_pane_was() {
    let (pwsh, wsl) = ("pwsh", "wsl");
    let chosen = PathBuf::from(r"D:\Developer\folio-terminal");
    let pane = PathBuf::from(r"C:\Users\dev\elsewhere");
    let carried = profiles::SeedPlace::Carried(pane.clone());

    assert_eq!(
        new_tab_cwd(pwsh, Some(&chosen), pwsh, Some(&carried)),
        Some(profiles::SeedPlace::Named(chosen.clone())),
        "the folder that was named out loud wins over the pane's own"
    );
    // The crossing is the chooser's, and the chooser speaks Windows: a WSL
    // tab opened on a Windows folder lands in WSL's spelling of it, which is
    // the same journey `StartAt::Fixed` makes at spawn.
    assert_eq!(
        new_tab_cwd(wsl, Some(&chosen), pwsh, Some(&carried)),
        Some(profiles::SeedPlace::Named(PathBuf::from(
            "/mnt/d/Developer/folio-terminal"
        ))),
    );
    // Nobody said: the pane answers, exactly as it did before this row
    // existed.
    assert_eq!(
        new_tab_cwd(pwsh, None, pwsh, Some(&carried)),
        Some(profiles::SeedPlace::Carried(pane.clone())),
    );
    assert_eq!(new_tab_cwd(pwsh, None, pwsh, None), None);

    // A cancelled chooser asks for nothing at all — no tab, no toast.
    assert_eq!(
        folder_pick_outcome(Some(FolderPick::NewTabIn), Ok(None)),
        None,
    );
    assert_eq!(
        folder_pick_outcome(Some(FolderPick::NewTabIn), Ok(Some(chosen.clone()))),
        Some((FolderPick::NewTabIn, chosen)),
    );
}

/// RED (GitHub issue #16) — **every road that names a folder for this launch opens the pane
/// there, whatever the profile's starting place says; every road that carries a folder keeps the
/// profile's rule.**
///
/// Each road is driven through the function the product builds its seed with — `cli::resolve`
/// and [`cli_leaf_seed`] for the first launch, [`new_tab_leaf_seed`] for every new tab (a second
/// launch handed over, a Service and a folder on the Dock all land through
/// `Runtime::new_tab_with_profile(&profile, request.cwd.clone())`, pinned by
/// `a_request_opens_its_tab_where_it_asked_and_raises_the_window`), [`SplitSeed::applied`],
/// [`restart_seed`] and [`revive_plan`] — and the seed is then placed by
/// `profiles::place_for`, the function `profiles::spawn_place` hands the table row to.
///
/// MUTATIONS, each observed red: drop `place_for`'s `Named` arm — every named road opens the
/// profile's folder under Home and Fixed; make `cli_leaf_seed` hand its folder over as
/// `SeedPlace::Carried` — the two first-launch rows; make `new_tab_cwd`'s chosen-folder branch
/// `Carried` — the three new-tab rows; make `SplitSeed::Folder` `Carried` — the pane row.
#[test]
fn every_road_that_names_a_folder_opens_there_whatever_the_profile_says() {
    struct Machine;
    impl bt_pty::ShellEnvironment for Machine {
        fn var_os(&self, _: &str) -> Option<std::ffi::OsString> {
            Some(r"C:\Users\用户".into())
        }
        fn is_file(&self, _: &Path) -> bool {
            false
        }
    }
    let home = Some(PathBuf::from(r"C:\Users\用户"));
    let fixed = PathBuf::from(r"E:\固定");
    let pwsh = "pwsh";
    let clicked = PathBuf::from(r"D:\项目\clicked");
    let pane = PathBuf::from(r"D:\elsewhere");
    let carried = profiles::SeedPlace::Carried(pane.clone());
    // A pane that was itself born in a named folder hands that folder on as named.
    let born_named = profiles::SeedPlace::Named(clicked.clone());
    let directory = |_: &Path| cli::PathKind::Directory;
    let handed_over = launch_wire::LaunchRequest {
        cwd: Some(clicked.clone()),
        tab: true,
        ..launch_wire::LaunchRequest::default()
    };
    let named: Vec<(&str, LeafSeed)> = vec![
        (
            "Open in Folio, --cwd, folio-here.cmd (first launch)",
            cli_leaf_seed(&cli::resolve(
                &cli::CliRequest {
                    cwd: Some(clicked.clone()),
                    origin: cli::LaunchOrigin::Explorer,
                    ..cli::CliRequest::default()
                },
                Some(profiles::index_of_id(pwsh)),
                directory,
            )),
        ),
        (
            "folio <folder> (first launch)",
            cli_leaf_seed(&cli::resolve(
                &cli::CliRequest {
                    path: Some(clicked.clone()),
                    ..cli::CliRequest::default()
                },
                Some(profiles::index_of_id(pwsh)),
                directory,
            )),
        ),
        (
            "a second launch, a Service, a folder on the Dock",
            new_tab_leaf_seed(pwsh, handed_over.cwd.as_deref(), pwsh, Some(&carried)),
        ),
        (
            "New terminal in folder… (new tab), New terminal here",
            new_tab_leaf_seed(pwsh, Some(&clicked), pwsh, Some(&carried)),
        ),
        (
            "New terminal in folder… (pane)",
            SplitSeed::Folder(clicked.clone()).applied(pwsh, Some(&carried)),
        ),
        (
            "Duplicate tab of a pane born in a named folder",
            new_tab_leaf_seed(pwsh, None, pwsh, Some(&born_named)),
        ),
        (
            "Duplicate pane and a split of a pane born in a named folder",
            SplitSeed::Inherit.applied(pwsh, Some(&born_named)),
        ),
        (
            "Split with, from a pane born in a named folder",
            SplitSeed::Profile(pwsh.to_owned()).applied(pwsh, Some(&born_named)),
        ),
        (
            "Restart shell of a pane born in a named folder",
            restart_seed(pwsh, Some(born_named.clone())),
        ),
    ];
    let place = |seed: &LeafSeed, start_at: &profiles::StartAt| {
        profiles::place_for(
            start_at,
            &profiles::StartingDir::AccountHome,
            profiles::PathNamespace::Windows,
            seed.cwd.clone(),
            &Machine,
        )
        .working_directory
    };
    let start_ats = [
        profiles::StartAt::Inherit,
        profiles::StartAt::Home,
        profiles::StartAt::Fixed(fixed.clone()),
    ];
    for (road, seed) in &named {
        for start_at in &start_ats {
            assert_eq!(
                place(seed, start_at),
                Some(clicked.clone()),
                "{road} under {start_at:?}: the folder named for this launch wins"
            );
        }
    }
    // The carried roads keep the profile's rule: Duplicate tab, the `+`, a split, Restart shell
    // and a restored session start where the pane stood only under `Inherit`.
    let here = std::env::current_dir().expect("a test runs somewhere");
    let (seats, _, restored, _files, _preview) =
        revive_plan(&saved_row_of_two(&here.to_string_lossy(), ""));
    let carried: Vec<(&str, LeafSeed)> = vec![
        (
            "the + and Duplicate tab",
            new_tab_leaf_seed(pwsh, None, pwsh, Some(&carried)),
        ),
        (
            "Duplicate pane and a split",
            SplitSeed::Inherit.applied(pwsh, Some(&carried)),
        ),
        (
            "Split with",
            SplitSeed::Profile(pwsh.to_owned()).applied(pwsh, Some(&carried)),
        ),
        ("Restart shell", restart_seed(pwsh, Some(carried.clone()))),
        (
            "a restored session",
            restored[&seats.terminals()[0]].clone(),
        ),
    ];
    for (road, seed) in &carried {
        let stood = seed.cwd.as_ref().map(|place| place.path().to_path_buf());
        assert!(stood.is_some(), "{road} carries a folder");
        assert_eq!(place(seed, &start_ats[0]), stood, "{road} under Inherit");
        assert_eq!(place(seed, &start_ats[1]), home, "{road} under Home");
        assert_eq!(
            place(seed, &start_ats[2]),
            Some(fixed.clone()),
            "{road} under a fixed folder"
        );
    }
}

/// PIN — C25: `cwdLeaf` (mock-up line 2585) is a walk over the path's own
/// text, which is why a drive root keeps a name where `Path::file_name`
/// gives none.
#[test]
fn the_cwd_layer_names_the_folder_you_are_standing_in() {
    for (path, leaf) in [
        (r"D:\Developer\folio-terminal", "folio-terminal"),
        (r"D:\Developer\folio-terminal\", "folio-terminal"),
        (r"D:\Developer\folio-terminal\\", "folio-terminal"),
        (r"C:\", "C:"),
        (r"\\server\share\work", "work"),
        ("/home/alice/src", "src"),
    ] {
        assert_eq!(
            display_title(
                None,
                None,
                Some(Path::new(path)),
                profiles::title(profiles::fallback_profile()),
                &profiles::announcement_set(profiles::fallback_profile()),
            ),
            leaf,
            "cwd {path}"
        );
    }
}

/// PIN — C26: a program-controlled title is untrusted input. It is stripped
/// of C0 and C1, trimmed, and capped at `TITLE_MAX`, because "in the product
/// it must also never be able to impersonate chrome" (mock-up line 2601).
///
/// Red gate: OSC 2 text used to reach the strip byte for byte — a title of
/// `"\u{1b}[2J"` or eighty characters of anything was drawn as given.
#[test]
fn a_program_title_is_stripped_and_capped_before_it_reaches_the_strip() {
    assert_eq!(
        display_title(
            None,
            Some("a\u{7}b\u{1b}c"),
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        "abc",
        "C0 goes, including the escape that starts every sequence"
    );
    assert_eq!(
        display_title(
            None,
            Some("\u{9b}0m evil"),
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        "0m evil",
        "and C1 goes, including the single-byte CSI"
    );
    assert_eq!(
        display_title(
            None,
            Some("  \tspaced  "),
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        "spaced",
        "the trim happens after the strip, as `cleanTitle` writes it"
    );
    assert_eq!(
        display_title(
            None,
            Some(&"x".repeat(80)),
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        )
        .chars()
        .count(),
        TITLE_MAX_CHARS,
        "forty characters, and the forty-first is not a title"
    );
    // A layer that sanitises to nothing has said nothing, and falls through
    // — otherwise a program could blank a tab with one control byte.
    assert_eq!(
        display_title(
            None,
            Some("\u{1}\u{2}"),
            Some(Path::new(r"C:\work")),
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        "work"
    );
    assert_eq!(
        display_title(
            None,
            Some(""),
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        profiles::title(profiles::fallback_profile())
    );
    assert_eq!(
        display_title(
            Some("hi\u{0}there"),
            Some("prog"),
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        "hithere",
        "the name you type goes through the same sieve (mock-up line 5882)"
    );
    assert_eq!(
        display_title(
            Some("   "),
            Some("prog"),
            None,
            profiles::title(profiles::fallback_profile()),
            &profiles::announcement_set(profiles::fallback_profile()),
        ),
        "prog",
        "emptying the override reveals the layer underneath"
    );
}

/// RED — **Audit 3, finding C-3: `ESC[24;8~` goes to a prompt the shell opened in order, and a
/// prompt mark printed by something running inside a command is not one.**
///
/// The chord's two gates were the pane's `Integration` — derived from the start program's file
/// name, so every Windows default pane passes it whether or not `folio.ps1` was ever loaded — and
/// an open input region. Both were writable by the child: `OSC 133` is in band and
/// unauthenticated, so seven bytes in a file reaching the screen through `cat`, in a git author
/// name, or in a compromised host's motd opened the region *while the real command was still
/// running*, and the next resize wrote `ESC[24;8~` onto the stdin of whatever that command was.
/// This window's own record says what that does to one of them: GNU readline inserts what it
/// cannot decode, and the next command dies on `syntax error near unexpected token ';'`.
/// `docs/DESIGN.md` line 2338 allows forged marker cycles, and scopes what it is accepting in the
/// same breath — "没有任何东西被执行". Bytes on a program's stdin are past that line.
///
/// The nested arm is the limit, stated rather than hidden: a shell started *by* a command speaks
/// the protocol legitimately and is byte-for-byte a program printing the same cycle. Its prompt is
/// marked like any other — that is the design's ruling and it still holds — and this window types
/// at neither, because the command it was started by has not ended.
#[test]
fn a_prompt_mark_forged_inside_a_running_command_is_typed_at_by_nothing() {
    let start = Instant::now();
    let resize_and_take = |session: &mut DualPlaneSession| {
        let mut pending = false;
        let integration = profiles::Integration::PowerShellOptIn;
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

    // The behaviour being kept: a real prompt is repaired, once.
    let mut real = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    real.feed_at(b"\x1b]133;A\x07PS> \x1b]133;B\x07", start)
        .unwrap();
    assert_eq!(
        resize_and_take(&mut real),
        Some(PSREADLINE_INVOKE_PROMPT_INPUT),
        "the pane the chord was cut for, at the prompt its own shell opened"
    );

    // A command is running and its output carries the marker.
    let mut forged = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    forged
        .feed_at(
            b"\x1b]133;A\x07PS> \x1b]133;B\x07ssh host\r\x1b]133;C\x07\r\n",
            start,
        )
        .unwrap();
    forged.feed_at(b"motd\x1b]133;B\x07", start).unwrap();
    assert!(
        forged.shell_input_region_open(),
        "the region still opens — a mark is owed to different evidence than a pty write"
    );
    assert_eq!(
        resize_and_take(&mut forged),
        None,
        "zero bytes reach a program the reader is talking to"
    );

    // A `D` this session's `C` never asked for, then a `B`: twenty bytes, no `A`, no prompt.
    let mut two_markers = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    two_markers
        .feed_at(
            b"\x1b]133;A\x07PS> \x1b]133;B\x07ssh host\r\x1b]133;C\x07\r\n",
            start,
        )
        .unwrap();
    two_markers
        .feed_at(b"motd\x1b]133;D;0\x07\x1b]133;B\x07", start)
        .unwrap();
    assert_eq!(
        resize_and_take(&mut two_markers),
        None,
        "a region with no `A` in front of it is not a prompt this window types at"
    );

    // A nested shell that speaks the protocol: marked, and still not typed at.
    let mut nested = DualPlaneSession::new(nonzero_u32(80), nonzero_u32(24));
    nested
        .feed_at(
            b"\x1b]133;A\x07PS> \x1b]133;B\x07pwsh\r\x1b]133;C\x07\r\n",
            start,
        )
        .unwrap();
    nested
        .feed_at(b"\x1b]133;A\x07nested> \x1b]133;B\x07", start)
        .unwrap();
    assert!(
        nested.shell_input_region_open(),
        "the nested shell's prompt is marked like any other (DESIGN.md line 2338)"
    );
    assert_eq!(
        resize_and_take(&mut nested),
        None,
        "and the command it was started by has not ended, so nothing is typed at it"
    );

    // That command ends, the pane's own shell prompts again, and the chord is owed again.
    nested
        .feed_at(
            b"exit\r\x1b]133;D;0\x07\x1b]133;A\x07PS> \x1b]133;B\x07",
            start,
        )
        .unwrap();
    assert_eq!(
        resize_and_take(&mut nested),
        Some(PSREADLINE_INVOKE_PROMPT_INPUT),
        "the shell this window spawned is reading a line again"
    );
}

/// PIN (ticket #62) — **the restart's inputs are the seat's own profile and
/// its last reported folder** (`docs/M2-restart-shell-contract.md` §1.1).
///
/// Written as a test of the seed rather than of a spawn, because the seed is
/// the whole of what the contract constrains: what happens to it afterwards
/// is `create_leaf_session`'s, and it is the same path a split and a revived
/// tab already take. What could silently go wrong is the *reading* — a
/// profile taken from the current default, or a directory asked of the dying
/// process — and both of those are visible here and nowhere else.
///
/// MUTATION: hand it a default profile instead of the leaf's and the first
/// assertion names the shell the pane would have come back as.
#[test]
fn a_restart_carries_the_seats_own_profile_and_its_last_reported_folder() {
    let profile = "gitbash";
    let reported = PathBuf::from(r"D:\Developer\folio-terminal");

    let seed = restart_seed(
        profile,
        Some(profiles::SeedPlace::Carried(reported.clone())),
    );
    assert_eq!(
        seed.profile, profile,
        "the seat's own profile, never the current default"
    );
    assert_eq!(
        seed.cwd,
        Some(profiles::SeedPlace::Carried(reported.clone())),
        "carried, so a restart keeps the profile's starting rule"
    );
    assert_eq!(
        seed.unknown_profile_id, None,
        "a pane that is running has a profile this build has"
    );

    // A pane with no place at all is an absence, not a fallback: `spawn_place`
    // answers `None` with the profile's own starting directory, which is what a
    // brand-new pane on that profile gets.
    assert_eq!(restart_seed(profile, None).cwd, None);
}

/// RED (T-RESTART-CWD round 2, finding 2) — **a leaf records where its shell was born in the
/// namespace of the profile that actually started.**
///
/// `bt-pty` falls back once to the last-resort shell when the profile's program will not start,
/// and `create_leaf_session` then makes the leaf the fallback profile — while its `spawn_place`
/// was resolved for the profile that was asked for. A WSL pane that came up as PowerShell held
/// `/mnt/d/Projects`, which the ladder now hands to the next PowerShell.
///
/// MUTATION, observed red: return the place and the mark unchanged after a swap — the WSL
/// spelling and the launcher's mark stay on a PowerShell leaf.
#[test]
fn a_shell_that_fell_back_records_its_birth_place_in_its_own_namespace() {
    let (wsl, fallback) = ("wsl", profiles::fallback_profile_id());
    assert_eq!(
        birth_place_of_the_started_shell(
            wsl,
            fallback,
            Some(PathBuf::from("/mnt/d/Projects")),
            false
        ),
        (Some(PathBuf::from(r"D:\Projects")), false),
        "crossed into the started profile's spelling"
    );
    assert_eq!(
        birth_place_of_the_started_shell(wsl, fallback, Some(PathBuf::from("~")), true),
        (None, false),
        "the launcher's home mark has no spelling there, and no mark goes with it"
    );
    assert_eq!(
        birth_place_of_the_started_shell(wsl, wsl, Some(PathBuf::from("~")), true),
        (Some(PathBuf::from("~")), true),
        "no swap, nothing changes"
    );
    let birth = free_fn_body("finish_leaf_birth");
    let swapped = birth
        .find("let profile = if let Some(fallback) = &shell_fallback {")
        .expect("the swap");
    let placed = birth
        .find("place_leaf(leaf, decision, seed, &profile, in_birth);")
        .expect("the leaf is placed for the started profile");
    assert!(swapped < placed, "{birth}");
    let place = free_fn_body("place_leaf");
    let said = place
        .find("birth_place_of_the_started_shell(")
        .expect("the birth place is said for the started profile");
    let told = place
        .find("leaf.session.set_spawn_at_shell_home(at_shell_home);")
        .expect("and the session is told the mark that goes with it");
    assert!(said < told, "{place}");
}

/// PIN — **a crash that nobody can see is a crash that nobody reports.**
///
/// `folio.exe` is a windows-subsystem binary and its `stderr` has been
/// redirected into a log file by the time anything can panic, so the default
/// hook's message reaches nobody: what a double-click user saw was the
/// window disappearing. The sentence raised in its place has to carry the
/// path, because a log file whose location is not on screen is a log file
/// nobody attaches.
#[test]
fn a_frame_shape_stop_records_its_cause_before_announcing() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/math-band-stop-tests")
        .join(bt_testpath::unique_name("frame-shape-stop"));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("folio-panic.log");
    let error = anyhow::Error::new(bt_viewport::FrameProjectionError::FrameShape(
        bt_viewport::FrameShapeError::MathBlockBandTop {
            expected: 643_072,
            actual: 688_128,
        },
    ))
    .context("project terminal grid into viewport frame");
    let mut announced = false;
    report_frame_shape_stop(&error, &path, |announced_path| {
        assert_eq!(announced_path, path);
        let report = std::fs::read_to_string(announced_path).unwrap();
        assert!(report.contains(&version::banner()));
        assert!(report.contains("stopped: project terminal grid into viewport frame"));
        assert!(report.contains("live math block top is 688128 subpixels, band starts at 643072"));
        assert!(report.contains("backtrace:"));
        announced = true;
    });
    assert!(announced);
    std::fs::remove_file(&path).unwrap();
    report_frame_shape_stop(&anyhow!("ordinary operational error"), &path, |_| {
        panic!("an unrelated stop must not be classified as a frame invariant failure");
    });
    assert!(!path.exists());
}

/// PIN — U8. A window resized mid-flight owes exactly the frames it draws.
///
/// The debt is settled against the **live** solve, passed in, and never
/// against a copy taken when the tween started. Nothing *drawn* depends on
/// this — both seams apply the running transform to whatever rectangle the
/// solver gave this frame, which is CSS's own rule that a transform is
/// relative to the box the element is currently laid out in — so the whole
/// cost of getting it wrong is a present that draws nothing new. That is
/// exactly what this counts.
///
/// The stale rectangle is not simulated by a mutation: it is the same call
/// with the pre-resize layout handed in, which is precisely what a stored
/// copy would have been. So the second half is the red gate, and it fails if
/// the two ever stop being distinguishable.
#[test]
fn a_window_resized_mid_flight_owes_exactly_the_frames_it_draws() {
    let now = Instant::now();
    let seat = SeatId(1);
    let before = [(seat, [0.0, 0.0, 700.0, 700.0])];
    let solved = [(seat, [0.0, 0.0, 500.0, 500.0])];
    // The same pane after the window was dragged much narrower, mid-flight:
    // every rectangle is re-solved and the tween decays onto the new one.
    // A pure shrink is deliberate — with no translation left, the *only*
    // thing deciding which frames draw something new is the extent the scale
    // multiplies, which is exactly the term a stored rectangle gets wrong.
    let resized = [0.0, 0.0, 150.0, 120.0];
    let live = [(seat, resized)];

    // The frames on which the seams actually put something new on screen:
    // the running transform over the rectangle the window *now* has.
    let mut reference = PaneMotion::default();
    reference.begin(&before, &solved, now, Motion::Full);
    let mut redraws = Vec::new();
    let mut last: Option<[i32; 4]> = None;
    let mut at = now;
    loop {
        let moving = reference.is_animating(at, Motion::Full);
        let showing = reference
            .transform_of(seat, at, Motion::Full)
            .applied_to(resized)
            .map(|edge| edge.round() as i32);
        redraws.push(last != Some(showing));
        last = Some(showing);
        if !moving {
            break;
        }
        at += pace::DEFAULT_FRAME_INTERVAL;
    }
    assert!(
        redraws.iter().filter(|drew| **drew).count() > 3,
        "the flight has to draw several distinct boxes, or this counts nothing"
    );

    let walk = |layout: &[(SeatId, [f32; 4])]| {
        let mut motion = PaneMotion::default();
        motion.begin(&before, &solved, now, Motion::Full);
        let mut owed = Vec::new();
        let mut at = now;
        loop {
            let moving = motion.is_animating(at, Motion::Full);
            owed.push(motion.settle_frame_debt(layout, at, Motion::Full));
            if !moving {
                break;
            }
            at += pace::DEFAULT_FRAME_INTERVAL;
        }
        (owed, motion)
    };

    let (owed, settled) = walk(&live);
    assert_eq!(
        owed, redraws,
        "a present is owed on exactly the frames something new is drawn — no \
             frame of the flight is skipped, and none of them pays twice"
    );
    assert_eq!(
        settled
            .entry(seat)
            .and_then(|pane| pane.last_drawn)
            .map(|drawn| drawn.box_px),
        Some(resized.map(|edge| edge.round() as i32)),
        "and it ends recorded as drawn in the rectangle the window actually \
             has, rather than in the one it had when the split happened"
    );

    // The red gate, and it is not a simulation: handing in the pre-resize
    // layout *is* what a rectangle stored at `begin` would have been.
    let (stale, stale_motion) = walk(&before);
    assert_ne!(
        stale, redraws,
        "the pre-resize rectangle really does owe a different set of frames, \
             so this pin can fail"
    );
    assert_ne!(
        stale_motion
            .entry(seat)
            .and_then(|pane| pane.last_drawn)
            .map(|drawn| drawn.box_px),
        Some(resized.map(|edge| edge.round() as i32)),
        "and it really would end up recording a box nobody drew"
    );
}

/// And the one this column wants — [`markdown_image_extent`]'s answer for
/// that native size in that measure, which is what the page asks the lane
/// for.
const SHARP: [u32; 2] = [800, 600];

/// RED — **a decode asked for to sharpen a picture is awaited, and its
/// arrival sharpens it** (adversarial review 2026-09-11, row RB-6;
/// §7.1.3u ③).
///
/// RED EVIDENCE. A page holding a soft raster whose decode the store has let
/// go asks for the file again — it cannot make the exact-size pass without
/// the pixels it would be resampled from. That ask set `needs_pixels` and
/// nothing else: `DocumentPictures::loading` was written only when the
/// answer was [`MarkdownPicture::Loading`], and the awaited set is built from
/// `loading`, so the page was in neither set a completion is delivered
/// against. The decode landed in the store, nobody was owed anything, and
/// the picture stayed soft until the reader happened to touch something.
///
/// MUTATION: drop [`PictureWaitFor::Sharpening`] from `answer_one_picture`'s
/// `None if !held_exactly` arm and the page names no file: the store fills,
/// the resample is never owed, and the page is still drawing its 240×180
/// raster in a 800-pixel column.
#[test]
fn a_sharpening_decode_is_awaited_and_its_completion_sharpens_the_picture() {
    let mut fixture = a_page_that_wants_a_sharper_picture();
    assert_eq!(
        fixture.reads, 1,
        "the page asked for the pixels it cannot resample from"
    );
    assert_eq!(
        fixture.page.awaited().count(),
        0,
        "it is not waiting to see the picture — it is drawing it: {:?}",
        fixture.page.waiting,
    );
    let (_, wanted) = fixture
        .page
        .sharpening()
        .find(|(file, _)| *file == &fixture.file)
        .expect("and it wrote down what that read is for");
    assert_eq!(
        (wanted.width_px, wanted.height_px),
        (SHARP[0], SHARP[1]),
        "the exact-size raster the column wants"
    );

    // The decode lands. Nothing on the glass changes, and the page is owed
    // one Lanczos3 pass.
    let native_rgba: Arc<[u8]> = Arc::from(
        vec![0_u8; (SHARPEN_NATIVE[0] as usize) * (SHARPEN_NATIVE[1] as usize) * 4]
            .into_boxed_slice(),
    );
    assert!(
        owe_sharpened_rasters(
            &mut fixture.rasters,
            std::iter::once(&fixture.page),
            &bt_term::normalized_local_image_path_key(&fixture.file),
            SHARPEN_CONTENT,
            &native_rgba,
            SHARPEN_NATIVE,
            Instant::now(),
        ),
        "the completion is delivered to the page that was waiting for it"
    );
    let owed = fixture
        .rasters
        .owed
        .values()
        .next()
        .expect("one exact-size pass, at the size the page wrote down")
        .clone();
    assert_eq!(
        (owed.key.width_px, owed.key.height_px),
        (SHARP[0], SHARP[1]),
        "and it is the pass the page asked for"
    );

    // The lane answers. The page is holding a sharp picture, and the decode
    // store never had to be asked a second time.
    fixture.rasters.land(
        owed.key.clone(),
        MarkdownRaster::Ready {
            key: bt_term::display_texture_key(&owed.key.content, SHARP[0], SHARP[1]),
            rgba: Arc::from(
                vec![0_u8; (SHARP[0] as usize) * (SHARP[1] as usize) * 4].into_boxed_slice(),
            ),
            width_px: SHARP[0],
            height_px: SHARP[1],
        },
    );
    let held = fixture.page.clone();
    let sharpened = resolve_sharpening_page(
        &mut fixture.peek,
        &mut fixture.rasters,
        &held,
        &mut fixture.reads,
    );
    assert_eq!(fixture.reads, 1, "and no second read went out for it");
    assert!(
        matches!(
            sharpened.get("shots/one.png"),
            Some(MarkdownPicture::Ready { raster, .. }) if *raster == SHARP
        ),
        "the picture is drawn at the size the column gives it: {:?}",
        sharpened.get("shots/one.png"),
    );
}

/// PIN — §7.1.4's whole ladder: a tab that never heard an OSC 7 is seeded
/// with **the place its shell was put down**, not with nothing.
///
/// Red gate, and the visible half of it is a row in `RECENTLY OPENED` with
/// no caption: `cwd_leaf("")` is the empty string, so closing a tab whose
/// shell had not yet reported a folder — every `cmd.exe` tab, every shell
/// with no integration script, and every tab closed inside the first second
/// of its life — put a blank line in the menu. `Restart shell` has answered
/// this with three rungs since §7.1.4 was written; this door had one.
#[test]
fn a_tab_that_never_reported_a_folder_is_seeded_where_its_shell_was_put_down() {
    let started_in = PathBuf::from(r"C:\Users\dev");
    let tab = tab_holding(LeafSession {
        spawn_place: Some(started_in.clone()),
        ..leaf_saying("no report from this one")
    });
    assert_eq!(
        tab.seed(),
        Some(seed::Seed::Term {
            profile_id: profiles::id(profiles::fallback_profile()),
            cwd: started_in.to_string_lossy().into_owned(),
            manual_name: None,
        }),
        "the second rung of the ladder, which the vault had never been told"
    );
    // And it is a row the vault will actually keep, which is the point of
    // computing it: the same tab used to seed an empty place and be refused.
    let mut vault = seed::SeedVault::default();
    vault.record(
        tab.seed().expect("a terminal tab seeds"),
        Vec::new(),
        SystemTime::UNIX_EPOCH,
    );
    assert_eq!(vault.len(), 1);

    // A report supersedes it the moment one arrives — the first rung is
    // still first, and it is the only one anybody actually *said*.
    let reported = tab_holding(LeafSession {
        spawn_place: Some(started_in.clone()),
        ..leaf_saying("\u{1b}]7;file://localhost/D:/Developer/folio-terminal\u{7}")
    });
    assert_eq!(
        reported.seed(),
        Some(seed::Seed::Term {
            profile_id: profiles::id(profiles::fallback_profile()),
            cwd: r"D:\Developer\folio-terminal".to_owned(),
            manual_name: None,
        }),
    );

    // And a machine that can name neither still says nothing rather than
    // guessing — the honest empty place, which the vault then declines to
    // store rather than drawing blank.
    let nowhere = tab_holding(leaf_saying("no report and no place"));
    assert_eq!(
        nowhere.seed(),
        Some(seed::Seed::Term {
            profile_id: profiles::id(profiles::fallback_profile()),
            cwd: String::new(),
            manual_name: None,
        }),
    );
    let mut vault = seed::SeedVault::default();
    vault.record(
        nowhere.seed().expect("a terminal tab seeds"),
        Vec::new(),
        SystemTime::UNIX_EPOCH,
    );
    assert!(
        vault.is_empty(),
        "an unwritable row is not written rather than written as a guess"
    );
}

/// PIN — K143, collapsed. **The menu's first row is one word.**
///
/// While there were two doors this row had to be named after whichever one
/// it would actually walk through, because a sentence can lie where a double
/// click cannot; `RowActivation::open_text` carried the promise that "the
/// day the second door closes this collapses to one word without anything
/// else moving". That day came, and the wording went the rest of the way
/// home: it is [`profiles::FileMenuRow::text`]'s `Open` arm, one string with
/// no branch and no caller able to supply another. A *second* row was then
/// opened beside it for the machine's own handler — and it is a second row
/// rather than a second wording of this one, which is what keeps this pin
/// true. This asserts what is left to assert: the row a file's menu opens
/// with, and the path its other rows act on, including for the rootless
/// column that has none.
#[test]
fn the_menus_first_row_says_one_thing_now_that_there_is_one_door() {
    let root = r"C:\work";
    assert_eq!(
        profiles::FileMenuRow::Open.text(&profiles::FileMenuLook {
            subject: profiles::FileMenuSubject::File,
            powers: file_menu_powers(Some(&FileMenuTreeRow {
                host: RowHost::Column(SeatId(1)),
                key: String::new(),
            })),
            crumbs: &[],
            terminal: marks::ChromeMark::ProfilePowerShell,
        }),
        "Open preview"
    );
    for name in ["/shot.png", "/notes.md", "/setup.exe"] {
        assert_eq!(
            files_row_activation(root, name).path(),
            Some(files::full_path(root, name).as_path()),
            "{name}"
        );
    }
    assert_eq!(
        files_row_activation("", "/notes.md").path(),
        None,
        "and a column with no root has nothing to put on a clipboard"
    );
}

/// RED GATE (§13.40) — **a pane head writes its folder the way this reader
/// writes one.** The sweep photographed a Mac window in which three
/// surfaces named one place: the rail said `~ › pages`, the files column's
/// foot said `~/pages`, and the terminal pane's head, a hundred physical
/// pixels under both, said
/// `/Users/<owner>/folio-port/wt/m2-7/out-acc/<the run’s own home>/pages` — while the shell
/// in that very pane printed `~` in its own prompt. §13.32 ③'s words for
/// `~` are "the character the shell in the pane below prints for the same
/// folder", and this head is the one directly above that shell.
///
/// Two halves, and the Windows half is the one a Windows gate can hold
/// outright: **nothing is substituted here**, so a head prints the string
/// the shell reported, byte for byte, on this machine. The Mac half is the
/// rule's own pin above; what ties the two together is that the head goes
/// *through* that rule rather than having one of its own, which is what the
/// source pin asserts.
///
/// MUTATIONS:
/// ① give `cwd_whole` its old `to_str()` body — the source pin goes red and
///    a Mac head goes back to printing the whole run above home;
/// ② substitute on Windows too — the first assertion goes red on this host;
/// ③ point the head at a second renderer beside `CWD_AS_WHOLE_PATH` — the
///    inventory that constant's doc keeps stops being the whole list, and
///    the source pin names the function that got around it.
#[test]
fn a_pane_heads_folder_is_written_the_way_this_reader_writes_one() {
    let write = CWD_AS_WHOLE_PATH.write;
    let home = profiles::home_directory(&bt_pty::SystemShellEnvironment)
        .expect("the reader running this test has a home directory");
    let under = home.join("notes");
    assert_eq!(
        write(&under).as_deref(),
        under.to_str(),
        "Windows knows no `~`, so a head prints what the shell reported"
    );
    assert_eq!(
        write(Path::new("/nowhere/at/all")).as_deref(),
        Some("/nowhere/at/all"),
        "a folder under nobody's home is printed whole on either machine"
    );
    // The Mac's half of the same claim, read from here because the rule
    // takes its platform and its home as values (§13.32 ③).
    assert_eq!(
        home_shortened_path_on(
            "/Users/alice/pages",
            bt_platform::HostPlatform::MacOs,
            Some(Path::new("/Users/alice")),
        ),
        "~/pages",
        "and a Mac's head says what a Mac's breadcrumbs say"
    );
    let body = free_fn_body("cwd_whole");
    assert!(
        body.contains("home_shortened_path"),
        "the head asks the breadcrumbs' own question rather than keeping a \
             second opinion about where a path starts — body was: {body}"
    );
}

/// **N157/K123 — the pane arrives in the strip running the shell it was
/// already running.**
///
/// The pin that says the gesture *moves* a session rather than trading one
/// for a fresh one. Two shells are given words nothing else in the window
/// has; after the tear-out the word that was in the right-hand pane is in
/// the new tab and the word that was in the left-hand pane is still in the
/// tab it left. A kill-and-respawn implementation passes every count in this
/// test and fails this one line, which is why it is here.
///
/// Everything else N157 and N158 rule is checked beside it: the tab that was
/// left keeps a shell per surviving leaf and no more (item 6), the new tab's
/// seat is re-minted from 1 with its session re-keyed to match, the profile
/// is inherited, the manual name is not (it belonged to the tab, not the
/// pane) and the new tab is unpinned even when the tab it came out of was
/// pinned — N158, because you aimed at the strip and made a new tab.
///
/// The landing wash is asserted here too, and it is asserted as a *tween in
/// flight* rather than as a field being set: `sample` is the only thing the
/// strip ever asks, so a `started` nothing samples to would be a wash that
/// exists in the struct and never on the glass. Its reduced-motion twin is
/// `reduced_motion_skips_the_landing_animation_outright`, which is why this
/// one hands `Motion::Full` explicitly instead of taking a default.
///
/// Red gate: spawn a new session for the torn pane instead of moving it and
/// the two content assertions go red; file it under the old `SeatId` and
/// `sessions_match_terminals` goes red; carry `pinned` across and N158 goes
/// red; drop the `landing.start` and the tab arrives with no wash at all.
#[test]
fn a_torn_out_pane_carries_its_own_shell_into_its_own_tab() {
    let mut source = cross_tab(1, &["ALPHA", "BETA"]);
    source.pinned = true;
    source.manual_name = Some("build".to_string());
    // Two panes, two different shells — the `[claude in mpc | git bash]`
    // shape `docs/UI-UX.md` §425 names, and the case a tab-level profile
    // could not express at all. The pane about to be torn out is the Git
    // Bash one, so "the same kind of shell it always was" has something to
    // be wrong about: under the old model the new tab took `from.profile`,
    // the tab's single answer, and a bash pane torn out of a PowerShell tab
    // arrived calling itself PowerShell.
    let gitbash = "gitbash";
    source
        .sessions
        .get_mut(&SeatId(2))
        .expect("the right-hand pane")
        .profile = gitbash.to_owned();
    assert_ne!(
        gitbash,
        profiles::fallback_profile_id(),
        "the two panes differ"
    );
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        SeatId(2),
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a two-pane tab can spare one");

    assert_eq!(
        leaf_says(&torn, SeatId(1)),
        "BETA",
        "the very session that was in the right-hand pane is in the new tab"
    );
    assert_eq!(
        leaf_says(&source, SeatId(1)),
        "ALPHA",
        "and the one that stayed is untouched"
    );
    assert_eq!(
        torn.seats.terminals(),
        vec![SeatId(1)],
        "the pane's ids are re-minted from 1 in the tab it now is"
    );
    assert_eq!(torn.focused_leaf, SeatId(1));
    assert!(torn.sessions_match_terminals(), "item 6, on the new tab");
    assert!(source.sessions_match_terminals(), "item 6, on the old one");
    assert_eq!(source.seats.terminals(), vec![SeatId(1)]);
    assert_eq!(
        torn.leaf_profile(SeatId(1)),
        gitbash,
        "the same kind of shell it always was — and it needs no copying, \
             because the profile rides on the session that moved"
    );
    assert_eq!(
        torn.tab_mark(&BTreeMap::new()),
        profiles::mark(profiles::index_of_id(gitbash)),
        "so the strip draws the new tab as the shell actually running in it"
    );
    assert_eq!(
        source.leaf_profile(SeatId(1)),
        profiles::fallback_profile_id(),
        "and the pane that stayed is still its own shell, not the one that left"
    );
    assert_eq!(
        torn.manual_name, None,
        "the name belonged to the tab, not to the pane"
    );
    assert!(
        !torn.pinned,
        "N158: you aimed at the strip and made a new tab, so it is unpinned"
    );
    // N157: it arrives wearing `.landing`, and it is still wearing it a
    // frame later — the wash is a 200ms flight, not a flag.
    let (wash, moving) = torn.landing.sample(Instant::now(), Motion::Full);
    assert!(
        moving && wash > 0.0,
        "the torn-out tab lands with the mock-up's wash still running: \
             {wash} / {moving}"
    );
}

/// **PIN — 「Move pane to new tab」两步路保留** (§7.1.6k's own sentence).
///
/// The spring-loaded drop is a second door onto moving a pane, never a
/// replacement for the menu row, and the row is what a hand that would rather
/// point twice than carry something across the window uses. Both doors run
/// one verb, which is why they cannot come to mean different things — and
/// that is what is asserted here rather than the row's mere existence: the
/// row is in the menu, it is spelled the way it always was, and
/// `move_pane_to_new_tab` still reaches `extract_pane_into_new_tab`.
///
/// Red gate: delete the row from `PaneMenuRow::ALL` and the first assertion
/// fails; give the row a second implementation and the source pin does.
#[test]
fn the_two_step_route_to_a_new_tab_is_untouched() {
    assert!(
        profiles::PaneMenuRow::ALL.contains(&profiles::PaneMenuRow::MoveToNewTab),
        "the row is still in the pane menu"
    );
    assert_eq!(
        i18n::Text::PaneMenuMoveToNewTab.text(),
        "Move pane to new tab",
        "and it still says what it always said"
    );
    let body = method_body("Runtime", "move_pane_to_new_tab");
    assert!(
        body.contains("self.extract_pane_into_new_tab(leaf, slot)"),
        "one verb, two doors: the row calls the same function the drag's \
             tear-out does, so a move can never quietly become a respawn"
    );
}

/// RED (35) — **the rail's `Open ⌄` peeks and pins like a chevron**: a rest
/// peeks and the hand leaving closes it; a press pins it and it stays; a
/// second press closes it.
///
/// The pill has been a `⌄` since 2026-09-10, and the ruling of 2026-09-23
/// names it with the other two. Its menu is the file menu, which a tree row's
/// right press and the breadcrumb's `…` chip also raise, and neither of those
/// is a chevron's: [`Runtime::chevron_menu_up`] governs the file menu only
/// while it is the pill's, and the press arm pins only what the pill opened.
///
/// MUTATION: delete `self.pin_the_chevron_menu_a_press_opened(Popup::File)`
/// from `press_preview_rail` and the source half goes red; make the rail gate
/// ignore its pin and the machine half does.
#[test]
fn the_open_pill_peeks_and_pins_like_a_chevron() {
    let (popup, pill) = the_three_chevrons()[2];
    let start = Instant::now();

    // Rest, peek, the hand leaves: closed after the grace.
    let mut gates = peek_open(popup, start);
    hand_leaves(&mut gates, popup, start);
    assert_eq!(
        gates.rail.due(start + profiles::CHEVRON_LEAVE_GRACE),
        Some(profiles::ChevronAction::Close)
    );
    gates.menu_gone(popup);

    // Rest again, press: pinned, and it stays however long the hand is away.
    let mut gates = peek_open(popup, start);
    assert_eq!(
        press_pins_a_peek(
            press_spends_itself_closing(chevron_button(pill), Some(pill)),
            gates.gate(popup),
        ),
        OwnPress::Pinned
    );
    for step in 1..=10u32 {
        let now = start + profiles::CHEVRON_LEAVE_GRACE * step;
        hand_leaves(&mut gates, popup, now);
        assert_eq!(gates.deadline(), None);
        assert_eq!(gates.rail.due(now), None);
    }

    // Press again: closed, and the pin is gone with it.
    assert_eq!(
        press_pins_a_peek(
            press_spends_itself_closing(chevron_button(pill), Some(pill)),
            gates.gate(popup),
        ),
        OwnPress::Spent
    );
    gates.menu_gone(popup);
    assert!(!gates.rail.is_pinned());

    // The doors: the pill's press pins what it opened; the gate governs only
    // the pill's file menu; the file menu's closers drop the pin.
    assert!(
        method_body("Runtime", "press_preview_rail")
            .contains("self.pin_the_chevron_menu_a_press_opened(Popup::File)")
    );
    assert!(method_body("Runtime", "chevron_menu_up").contains("menu.rail().is_some()"));
    for closer in ["close_file_menu", "run_file_menu_row", "open_file_menu"] {
        assert!(
            method_body("Runtime", closer).contains("self.window.chevrons.menu_gone(Popup::File)"),
            "{closer} takes the pill's pin with the menu it puts away or replaces"
        );
    }
}

/// PIN — **`Duplicate pane` carries the profile and the directory; `Split
/// with` carries the directory across the namespaces; the chooser's folder
/// carries the profile.**
///
/// One test over [`SplitSeed`] because the three rows differ in exactly one
/// field each, and the interesting failure for all three is the same: a seed
/// that dropped a half and let the arriving shell fall back to the profile's
/// own default, which looks like a shell that opened somewhere odd rather
/// than like a bug.
///
/// The WSL crossing is the one that cannot be got right by accident: a
/// Windows `D:\repo` handed to `wsl.exe` unconverted names nothing, and the
/// pane opens at `~` with no explanation.
#[test]
fn a_seeded_split_carries_the_profile_and_the_directory_the_row_promised() {
    let (pwsh, wsl) = ("pwsh", "wsl");
    let here = PathBuf::from(r"D:\Developer");

    // Duplicate: both halves, unchanged.
    let same = SplitSeed::Inherit.applied(
        wsl,
        Some(&profiles::SeedPlace::Carried(PathBuf::from("/home/me/src"))),
    );
    assert_eq!(same.profile, wsl);
    assert_eq!(
        same.cwd,
        Some(profiles::SeedPlace::Carried(PathBuf::from("/home/me/src")))
    );

    // Split with… : the named profile, standing where this pane stands, in
    // the spelling the named profile can read.
    let crossed = SplitSeed::Profile(wsl.to_owned())
        .applied(pwsh, Some(&profiles::SeedPlace::Carried(here.clone())));
    assert_eq!(crossed.profile, wsl);
    assert_eq!(
        crossed.cwd,
        Some(profiles::SeedPlace::Carried(PathBuf::from(
            "/mnt/d/Developer"
        ))),
        "the directory crosses the namespace rather than being copied into it"
    );

    // A pane whose shell has never named a directory hands over nothing,
    // which is an absence rather than a guess.
    assert_eq!(
        SplitSeed::Profile(wsl.to_owned()).applied(pwsh, None).cwd,
        None
    );

    // New terminal in folder… : this pane's own profile, in the folder the
    // chooser answered with — and that answer is a Windows path, so it too
    // crosses when the pane is a WSL one.
    let folder = SplitSeed::Folder(here.clone()).applied(pwsh, None);
    assert_eq!(folder.profile, pwsh);
    assert_eq!(folder.cwd, Some(profiles::SeedPlace::Named(here.clone())));
    let folder_for_wsl = SplitSeed::Folder(here.clone()).applied(wsl, None);
    assert_eq!(folder_for_wsl.profile, wsl);
    assert_eq!(
        folder_for_wsl.cwd,
        Some(profiles::SeedPlace::Named(PathBuf::from(
            "/mnt/d/Developer"
        ))),
        "the chooser speaks Windows, and a WSL pane does not"
    );
}

/// RED (45) — **a block without the marks keeps `Run line by line` as its default**, and the other
/// shells' marks are their own.
///
/// A block of commands is what ticket 02's card was for, and it still runs line by line on
/// `Enter`. A mark is the target shell's: a backslash-wrapped block is wrapped for a POSIX shell
/// and not for cmd, and `^^` at a line's end is cmd's literal caret, not a continuation.
///
/// MUTATION: treat every block as wrapped (`continued_by` answering `true`) — the first
/// assertion goes red.
#[test]
fn a_block_without_marks_keeps_run_line_by_line_as_its_default() {
    let default_for = |grammar, text: &str| {
        let (mut tab, target) = paste_tab(paste_leaf(grammar, b""));
        assert_eq!(
            paste_text_into(&mut tab, target, text, true),
            StagedPaste::Held
        );
        let (_, pending) = pending_paste_in(&tab).expect("the card is up");
        (
            pending.default_answer(),
            paste_answer_text(pending, PasteAnswer::Join).expect("a join sends"),
        )
    };
    let cmd = shell_literal::ShellGrammar::Cmd;
    let posix = shell_literal::ShellGrammar::Posix;
    assert_eq!(
        default_for(cmd, THREE_LINES),
        (PasteAnswer::RunLineByLine, "dir echo one ver".to_owned())
    );
    assert_eq!(
        default_for(cmd, "echo ^^\r\nver").0,
        PasteAnswer::RunLineByLine,
        "`^^` is a literal caret"
    );
    assert_eq!(
        default_for(cmd, "ls \\\n  -la").0,
        PasteAnswer::RunLineByLine,
        "a backslash is not cmd's mark"
    );
    assert_eq!(
        default_for(cmd, "dir ^\n\nver").0,
        PasteAnswer::RunLineByLine,
        "a blank line ends no command with a mark"
    );
    assert_eq!(
        default_for(posix, "ls \\\n  -la \\\n  /tmp"),
        (PasteAnswer::Join, "ls -la /tmp".to_owned()),
        "a POSIX shell's mark is the backslash"
    );
}

/// One of the 2026-09-23 spike's recordings (clean Windows 11 VM, guest user `folio`): the raw
/// bytes a real PowerShell printed, and the byte offsets at which the harness wrote the paste and
/// then `\r`.
struct SpikeRecording {
    bytes: &'static [u8],
    phases: &'static str,
}

impl SpikeRecording {
    fn offset(&self, phase: &str) -> usize {
        self.phases
            .lines()
            .find_map(|line| {
                let mut fields = line.split('\t');
                (fields.next() == Some(phase)).then(|| fields.next().unwrap().parse().unwrap())
            })
            .unwrap_or_else(|| panic!("no `{phase}` in the phases"))
    }
}

/// RED (0.4.4 ticket 03) — **a recorded PowerShell block runs once, on one Enter.**
///
/// The seam from Folio's bytes to the shell's behaviour, over the spike's recordings: the real
/// producer stages the spike's three lines into a PowerShell prompt, and the bytes it writes pick
/// the recording of what a real shell did with exactly those bytes — `0x16` is arm A, today's
/// `\r`-joined text is the control arm. Replayed into a real session, the answer must be the
/// ruled one on all three shells measured: no `OSC 133;C` before the Enter, the prompt still
/// open, then exactly one `C` and one `D`.
///
/// MUTATION: answer `AsTyped` for `InputLine` in `stage_paste` — today's bytes pick the control
/// recording, where two commands ran before any Enter.
#[test]
fn a_recorded_powershell_block_runs_once_on_one_enter() {
    let text = "'one' + 'RAN'\r\n'two' + 'RAN'\r\n'three' + 'RAN'";
    let shells = [
        (
            "5.1 + PSReadLine 2.0.0",
            SpikeRecording {
                bytes: include_bytes!("../tests/fixtures/multiline-paste/ps51-psrl200-A.bin"),
                phases: include_str!("../tests/fixtures/multiline-paste/ps51-psrl200-A.phases"),
            },
            SpikeRecording {
                bytes: include_bytes!("../tests/fixtures/multiline-paste/ps51-psrl200-today.bin"),
                phases: include_str!("../tests/fixtures/multiline-paste/ps51-psrl200-today.phases"),
            },
        ),
        (
            "5.1 + PSReadLine 2.4.6",
            SpikeRecording {
                bytes: include_bytes!("../tests/fixtures/multiline-paste/ps51-psrl246-A.bin"),
                phases: include_str!("../tests/fixtures/multiline-paste/ps51-psrl246-A.phases"),
            },
            SpikeRecording {
                bytes: include_bytes!("../tests/fixtures/multiline-paste/ps51-psrl246-today.bin"),
                phases: include_str!("../tests/fixtures/multiline-paste/ps51-psrl246-today.phases"),
            },
        ),
        (
            "pwsh 7.6.6 + PSReadLine 2.4.5",
            SpikeRecording {
                bytes: include_bytes!("../tests/fixtures/multiline-paste/pwsh7-A.bin"),
                phases: include_str!("../tests/fixtures/multiline-paste/pwsh7-A.phases"),
            },
            SpikeRecording {
                bytes: include_bytes!("../tests/fixtures/multiline-paste/pwsh7-today.bin"),
                phases: include_str!("../tests/fixtures/multiline-paste/pwsh7-today.phases"),
            },
        ),
    ];
    for (shell, clipboard_road, todays_road) in shells {
        let (mut tab, target) = paste_tab(paste_leaf(
            shell_literal::ShellGrammar::PowerShell,
            POWERSHELL_PROMPT,
        ));
        let staged = paste_text_into(&mut tab, target, text, true);
        let sent = staged_bytes_sent(&mut tab, target.seat, &staged).expect("nothing is held");
        let recording = if sent == b"\x16" {
            &clipboard_road
        } else if sent == input::paste_bytes(text, false) {
            &todays_road
        } else {
            panic!("{shell}: no recording of what a shell does with {sent:?}");
        };
        let mut replay =
            DualPlaneSession::new(NonZeroU32::new(120).unwrap(), NonZeroU32::new(40).unwrap());
        let executed = |session: &DualPlaneSession| {
            session
                .command_marks()
                .iter()
                .filter(|mark| mark.executed.is_some())
                .count()
        };
        let finished = |session: &DualPlaneSession| {
            session
                .command_marks()
                .iter()
                .filter(|mark| mark.finished.is_some())
                .count()
        };
        let arm = recording.offset("arm");
        let enter = recording.offset("enter");
        let end = recording.offset("end");
        replay.feed(&recording.bytes[..arm]).unwrap();
        assert!(replay.shell_prompt_opened_in_order(), "{shell}: control");
        replay.feed(&recording.bytes[arm..enter]).unwrap();
        assert_eq!(executed(&replay), 0, "{shell}: a line ran before Enter");
        assert!(
            replay.shell_prompt_opened_in_order(),
            "{shell}: the block is not on an open prompt"
        );
        replay.feed(&recording.bytes[enter..end]).unwrap();
        assert_eq!(executed(&replay), 1, "{shell}: one Enter, one command");
        assert_eq!(finished(&replay), 1, "{shell}: one command ended");
    }
}
