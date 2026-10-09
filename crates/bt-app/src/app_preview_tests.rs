//! **The crate root: previews and documents.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    PtyPresentationHarness, TAB_ONE, a_held_raster, a_page_that_wants_a_sharper_picture,
    assert_close, buffer_saying, calls_of, card_text, cell_ink, cross_metrics, cross_move,
    cross_solve, document_key, engines_settling_to, found_in, free_calls_of, free_fn_body,
    host_path, in_product, inside, item_body, leaf_saying, ledger_gate, markdown_body, method_body,
    mono_caret_block, no_directories, one_picture, pictures_drawn, prose, prose_caret_block,
    reader_names, rested_bars, scale_task, seat_of, source, source_block, squeezed_body,
    tab_with_a_picture, tab_with_a_preview, text_buffer,
};
use bt_render::LIGHT_CHROME;
use bt_source::{ItemQuery, Pattern, Scope, View, needle};
use std::time::Duration;
use winit::keyboard::{Key, NamedKey};

/// **G-0 — the content section's bytes did not move when the identity did.**
///
/// [`preview::PreviewSource`] replaced a bare `PathBuf` everywhere a preview
/// is keyed, and the one place that change must not be visible is the file
/// on disk: a `session.json` written by the build before it has to read back
/// here, and one written here has to be the same bytes that build would have
/// produced. So the assertion is against the JSON itself rather than against
/// a struct — a struct comparison would go on passing through a rename, a
/// reordering, or a field that started serializing as an object.
///
/// The second half is the ruling the skeleton comes with: a buffer with no
/// file behind it is **left out** rather than spelled. The schema has no
/// field for a repository, and writing `"path": "git:C:\\w\\repo:src/main.rs"`
/// would put back the exact ambiguity the sum type was introduced to retire
/// — this time on disk, where it would outlive the process. Carrying git
/// buffers across a restart is the G-series' own question and needs a schema
/// field, not a string that looks like a path.
///
/// MUTATIONS: ① render a git source into `path` instead of skipping it (the
/// entry count and the JSON both go red); ② change the `File` arm to write
/// anything but the path verbatim — a normalised separator, a display name —
/// and the fixture goes red on the exact byte.
#[test]
fn the_content_section_writes_a_file_exactly_as_it_always_did() {
    let mut pool = preview::PreviewPool::default();
    pool.insert(preview::PreviewBuffer::new(
        preview::PreviewSource::file(r"D:\notes\README.md"),
        "README.md".to_owned(),
    ));
    // A dirty git buffer, which is the hardest case for "skipped": the pool
    // is full of it, the switcher lists it, and it still owns no path.
    let mut diff = preview::PreviewBuffer::new(
        preview::PreviewSource::GitDiff {
            root: PathBuf::from(r"C:\w\repo"),
            path: "src/main.rs".to_owned(),
            against: preview::GitDiffAgainst::Index,
        },
        "main.rs".to_owned(),
    );
    diff.dirty = true;
    pool.insert(diff);
    // And v2 ②'s range, which is skipped by the same one rule rather than by
    // a second one: it has no file behind it, so `file_path` answers `None`,
    // so nothing is written. There is no leaf token for it to add — no git
    // source has ever had one.
    pool.insert(preview::PreviewBuffer::new(
        preview::PreviewSource::GitDiffRange {
            root: PathBuf::from(r"C:\w\repo"),
            a: "a".repeat(40),
            b: None,
            path: "src/main.rs".to_owned(),
        },
        "main.rs".to_owned(),
    ));
    pool.insert(preview::PreviewBuffer::new(
        preview::PreviewSource::file(r"D:\notes\todo.txt"),
        "todo.txt".to_owned(),
    ));

    let mut seats = seats::Seats::lone_terminal();
    let seat = seats
        .add_preview(&cross_metrics())
        .expect("a preview lands");
    let focused = seats.identity();
    let mut panes = PreviewPanes::default();
    panes.entry(seat_of(TAB_ONE, seat)).buffer =
        Some(preview::PreviewSource::file(r"D:\notes\README.md"));
    let (layout, overflow) = cross_solve(&seats);
    let tab = assemble_tab_state(
        TabId(1),
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

    let written = tab.preview_content().expect("this tab has previews");
    assert_eq!(
        written.pool.len(),
        2,
        "the two files, and not the git buffer standing between them"
    );
    assert_eq!(
        serde_json::to_string(&written).expect("the section serializes"),
        r#"{"panes":[{"leaf":"leaf-1","cur":"D:\\notes\\README.md"}],"pool":[{"path":"D:\\notes\\README.md","name":"README.md"},{"path":"D:\\notes\\todo.txt","name":"todo.txt"}]}"#,
        "byte for byte the shape a session written before PreviewSource had"
    );
}

/// T2/T3 (v2 ③) — the graph's branch filter goes to disk beside the pane it
/// belongs to, comes back on the pane it was written for, and is **absent**
/// from a session nobody filtered.
///
/// Three claims, and each is a way this could have been wrong: an unfiltered
/// graph must add no bytes at all (a `"graph":{"branches":[],...}` on every
/// pane would be a schema change every existing document paid for); a
/// filtered one must survive the round trip whole; and a document written
/// *before* this field must read as a graph with both checkboxes still
/// ticked, because the resting state of "Show tags" is on and a serde
/// default would have made it off.
#[test]
fn a_graphs_branch_filter_crosses_the_disk_and_an_unfiltered_one_writes_nothing() {
    let build_tab = |filter: Option<git_graph::GraphFilter>| {
        let mut seats = seats::Seats::lone_terminal();
        let seat = seats
            .add_preview(&cross_metrics())
            .expect("a preview lands");
        let focused = seats.identity();
        let (layout, overflow) = cross_solve(&seats);
        let mut views = BTreeMap::new();
        if let Some(filter) = filter {
            views.insert(
                seat_of(TAB_ONE, seat),
                GraphView {
                    filter,
                    ..GraphView::default()
                },
            );
        }
        (
            assemble_tab_state(
                TabId(1),
                BTreeMap::from([(focused, leaf_saying("SHELL"))]),
                BTreeMap::new(),
                preview::PreviewPool::default(),
                PreviewPanes::default(),
                views,
                focused,
                TabSeed::default(),
                seats,
                layout,
                overflow,
            ),
            seat,
        )
    };

    // ① An unfiltered graph writes exactly the bytes it always did.
    let (plain, _) = build_tab(Some(git_graph::GraphFilter::default()));
    let written = plain.preview_content().expect("this tab has a preview");
    assert_eq!(
        serde_json::to_string(&written).expect("the section serializes"),
        r#"{"panes":[{"leaf":"leaf-1","cur":null}],"pool":[]}"#,
        "an unfiltered graph is not a fact worth a field"
    );

    // ② A filtered one survives the round trip whole.
    let filter = git_graph::GraphFilter {
        branches: vec!["main".to_owned(), "side".to_owned()],
        remotes: false,
        tags: true,
    };
    let (filtered, seat) = build_tab(Some(filter.clone()));
    let written = filtered.preview_content().expect("this tab has a preview");
    let json = serde_json::to_string(&written).expect("the section serializes");
    assert!(
        json.contains(r#""graph":{"branches":["main","side"],"remotes":false,"tags":true}"#),
        "the filter is written in the reader's vocabulary, not git's: {json}"
    );
    let read: bt_persist::TabPreviewV1 = serde_json::from_str(&json).expect("and reads back");
    let restored = PreviewRestore::from_persisted(&filtered.seats, Some(&read));
    assert_eq!(restored.filters.get(&seat), Some(&filter));

    // ③ A document written before the field reads as an unfiltered graph
    // with **both** checkboxes still ticked.
    let old: bt_persist::TabPreviewV1 =
        serde_json::from_str(r#"{"panes":[{"leaf":"leaf-1","cur":null}],"pool":[]}"#)
            .expect("a session written before v2 (3) still reads");
    assert!(
        PreviewRestore::from_persisted(&filtered.seats, Some(&old))
            .filters
            .is_empty(),
        "no filter written is no filter restored, which is `All branches`"
    );
    // And the same, one level down: a `graph` object that names only the
    // branches leaves the two flags at the state their checkboxes rest in.
    let partial: bt_persist::GraphFilterV1 =
        serde_json::from_str(r#"{"branches":["main"]}"#).expect("an older shape reads");
    assert!(
        partial.remotes && partial.tags,
        "a serde default of `false` here would clear two checkboxes nobody unticked"
    );
}

/// PIN (next22 #206): **one clock, one rate, and the wrap is not a seam.**
///
/// The angle is a continuous monotone clock times a constant rate taken mod
/// one turn, and this is the arithmetic that says so: a run of frames at a
/// frame's own cadence, crossing twelve o'clock three times, where every
/// step is the same size as every other. Each of the four ways this goes
/// wrong shows up here as one step that is not the others — a phase that
/// restarts per turn closes short, an eased turn is fast in its middle and
/// slow at its ends, and a frame lost or repeated at the wrap is a step of
/// nothing or of two.
///
/// One milliturn of slack, and it is the *unit's* slack rather than the
/// clock's: an angle stated in thousandths cannot advance by 15.15 of them,
/// so consecutive steps of 15 and 16 are the same rate written down. It is
/// a third of a degree, and the defect it has to stay clear of was a
/// quarter of the circle.
///
/// The arc's *length* is checked here too, in the same loop, because the
/// second way a spin can seam is for the arc to breathe against the turn
/// and close its own period somewhere other than the top.
#[test]
fn the_spin_steps_the_same_angle_through_the_wrap() {
    let palette = LIGHT_CHROME;
    let period = Duration::from_millis(WINDOW_TAB_RING_SPIN_PERIOD_MS);
    // One frame of a 60Hz pane, which is the cadence the ring is actually
    // sampled at and no divisor of the period.
    let frame = Duration::from_micros(16_667);
    let step_milliturns = 1000.0 * frame.as_secs_f64() / period.as_secs_f64();
    // Three whole turns, so the wrap is crossed three times over and no
    // crossing is the first step of the run.
    let frames = (3.0 * period.as_secs_f64() / frame.as_secs_f64()).ceil() as u32;

    let at = |elapsed| {
        ring_arc(
            ProgressState::Indeterminate,
            None,
            elapsed,
            Motion::Full,
            &palette,
        )
    };
    let sweep = at(Duration::ZERO).sweep_milliturns;
    let mut previous = at(Duration::ZERO).start_milliturns;
    for frame_index in 1..=frames {
        let here = at(frame * frame_index);
        let stepped = (u32::from(here.start_milliturns) + 1000 - u32::from(previous)) % 1000;
        assert!(
            (f64::from(stepped) - step_milliturns).abs() <= 1.0,
            "frame {frame_index} turned {stepped} milliturns where every frame turns \
                 {step_milliturns:.2} — the angle is not one clock at one rate"
        );
        assert_eq!(
            here.sweep_milliturns, sweep,
            "frame {frame_index} changed the arc's length; an indeterminate arc has one"
        );
        previous = here.start_milliturns;
    }
}

/// RED (§7.1.5g ⑦, user ruling 2026-08-29) — **a link inside a preview reads
/// the same table as a link in the terminal**.
///
/// The debt §7.1.5g ⑥ booked, in its own words: a `http(s)` link in the body
/// of a markdown document went `preview::link_action` → `Browse` →
/// `shell_execute`, so **a plain press left for the system browser and that
/// path never read `control` at all**. It was written on 2026-08-13, four
/// months before [`ClickIntent`] existed, and by 2026-08-29 it was pointing
/// the opposite way from the rule the terminal had settled on: 平点 = 留在
/// 这扇窗里, `Ctrl`+点 = 交给系统. One product, one gesture, two answers —
/// which is the disagreement `ClickIntent` was minted to end, reappearing on
/// the surface nobody had gone back to.
///
/// So the row is asserted here exactly as it is asserted for the terminal
/// one screen up, and about the same strings: **the two surfaces are given
/// the same addresses and must answer the same way**. Since ticket 14 (owner
/// ruling 2026-09-23) that is true of the file arm too — 「click stays in the
/// window, Ctrl+click hands over」 — and of every other scheme.
///
/// MUTATION: put the old arm back — `LinkAction::Web(url) =>` the browser
/// under both modifiers, the 2026-08-13 verb that never read the modifier —
/// and every plain row below goes red while every `Ctrl` row stays green,
/// which is the shape of the debt exactly.
#[test]
fn a_link_inside_a_preview_reads_the_same_table_as_a_link_in_the_terminal() {
    let document = &host_path(r"D:\repo\docs\DESIGN.md");
    for uri in [
        "https://claude.ai/code/artifact/04c0a133-319b-4c8e-b988-7965fe063626",
        "https://github.com/openai/codex/releases/latest",
        "http://localhost:5173/index.html",
        "HTTPS://EXAMPLE.TEST/Path?q=1#frag",
    ] {
        assert_eq!(
            preview_link_activation(false, uri, document),
            HyperlinkActivation::Page(uri.to_owned()),
            "a plain click on {uri:?} in a document opens it in this window"
        );
        assert_eq!(
            preview_link_activation(true, uri, document),
            HyperlinkActivation::Browser(uri.to_owned()),
            "and Ctrl hands {uri:?} to the system"
        );
        // **The two surfaces, one answer.** Asserted as a pairing rather
        // than as two lists, because two lists that agree today is exactly
        // what these two were for four months.
        assert_eq!(
            hyperlink_activation(
                false,
                true,
                uri,
                bt_transcript::paths::PathNamer::ThisWindow,
                &no_directories
            ),
            HyperlinkActivation::Page(uri.to_owned()),
            "the terminal says the same about {uri:?}"
        );
        // The finger follows the verb on this surface too, and follows it
        // under **both** modifiers, because both halves of this row act.
        for control in [false, true] {
            assert!(
                preview_link_answers_a_press(control, uri, document),
                "the hand is on {uri:?} with control={control}"
            );
        }
        // The gate the plain half is spent through is the address bar's
        // own: `open_web_page`'s contract is that every caller passes
        // `webnav::address_bar` first, so this arm may only carry addresses
        // that door admits — otherwise a document and the address field
        // would give two answers about one string (§7.1.5g ⑤).
        let HyperlinkActivation::Page(url) = preview_link_activation(false, uri, document) else {
            panic!("{uri} is a page");
        };
        assert!(
            matches!(webnav::address_bar(&url), webnav::Decision::Navigate(_)),
            "the one door admits what the arm carries: {uri:?}"
        );
    }
    // **The file arm reads the modifier too** (owner ruling 2026-09-23). A
    // plain press is still 2026-08-13's answer — the seat, with a relative
    // target resolved against the document's own folder — and `Ctrl` hands
    // the same file to the machine, as it does from the terminal.
    for target in [
        "./notes.md",
        r"..\assets\a.png",
        "file:///C:/notes/a%20b.md",
    ] {
        let HyperlinkActivation::Preview(named, None) =
            preview_link_activation(false, target, document)
        else {
            panic!("{target:?} previews plainly");
        };
        assert_eq!(
            preview_link_activation(true, target, document),
            HyperlinkActivation::External(named),
            "and Ctrl hands the same file, {target:?}, to the system"
        );
        for control in [false, true] {
            assert!(preview_link_answers_a_press(control, target, document));
        }
    }
    // Any other scheme: nothing plainly, the machine's handler under `Ctrl`
    // (ticket 14). An anchor and an empty target name nothing and ask for
    // nothing, under either modifier.
    assert_eq!(
        preview_link_activation(false, "mailto:person@example.test", document),
        HyperlinkActivation::None
    );
    assert_eq!(
        preview_link_activation(true, "mailto:person@example.test", document),
        HyperlinkActivation::Scheme("mailto:person@example.test".to_owned())
    );
    assert!(!preview_link_answers_a_press(
        false,
        "mailto:person@example.test",
        document
    ));
    assert!(preview_link_answers_a_press(
        true,
        "mailto:person@example.test",
        document
    ));
    for target in ["#section", "   "] {
        for control in [false, true] {
            assert_eq!(
                preview_link_activation(control, target, document),
                HyperlinkActivation::None,
                "{target:?} does nothing with control={control}"
            );
            assert!(
                !preview_link_answers_a_press(control, target, document),
                "and wears no hand: {target:?} control={control}"
            );
        }
    }
    // An address whose own text does not parse: silent plainly, blocked
    // under `Ctrl` — the terminal's answer, arrived at through the
    // terminal's own reading of that row.
    assert_eq!(
        preview_link_activation(false, "http://", document),
        HyperlinkActivation::None
    );
    assert_eq!(
        preview_link_activation(true, "http://", document),
        HyperlinkActivation::Blocked(LinkRefusal::Invalid)
    );
    assert!(!preview_link_answers_a_press(false, "http://", document));
    assert!(preview_link_answers_a_press(true, "http://", document));
    // **A plain press the door refuses owes the reader a sentence.** This
    // address parses as text — so the plain half really does reach the
    // `Page` arm and really does make a request — and `webnav::address_bar`
    // refuses it as the phishing shape. That pair is what makes the refusal
    // reachable at all, and therefore what makes 「被拒出声」 a thing this
    // window has to do rather than a case it can never be in.
    let phishing = "https://claude.ai@evil.test/code";
    assert_eq!(
        preview_link_activation(false, phishing, document),
        HyperlinkActivation::Page(phishing.to_owned())
    );
    assert!(matches!(
        webnav::address_bar(phishing),
        webnav::Decision::Refuse(_)
    ));
}

/// RED (39) — **A page on an http address is handed to the browser as that address.**
///
/// A web pane on `http://127.0.0.1:8732/…` drew the `↗` in its head and in its address row, and
/// pressing either did nothing: the press asked only for the file under the page, and an `http`
/// page has none, so the handler returned before handing anything (owner, 2026-09-23:
/// 「这个肯定是要能打开的」). The seat's source is minted by the producer the window commits a page
/// with (`webnav::switcher_key`), and the request is the one `Runtime::open_preview_in_browser`
/// puts on the OS hand-off lane — one `Handoff::Address`, the whole address, query and fragment
/// included, and nothing else.
///
/// MUTATION: delete the new arm — answer `None` in `preview_page_browser_hand_off` where it maps
/// `source.web_url()` to `PageHandOff::Address` — and no page on `http` hands anything.
#[test]
fn a_page_on_an_http_address_is_handed_to_the_browser_as_that_address() {
    let surface = seat_of(TAB_ONE, SeatId(1));
    for url in [
        "http://127.0.0.1:8732/x.html",
        "http://127.0.0.1:8732/design05/wireframe/menus-b.html",
        "https://example.test/manual.pdf",
        "https://example.test/a.html?q=1&r=two#section-3",
        "http://localhost:5173/app",
    ] {
        let source = preview::PreviewSource::Web(webnav::switcher_key(url));
        let Some(PageHandOff::Address(address)) = preview_page_browser_hand_off(&source) else {
            panic!("a page with no file behind it is handed over by its address: {url:?}");
        };
        let (request, _) = page_address_hand_off(surface, &address);
        assert_eq!(
            request,
            bt_platform::Handoff::Address(url.to_owned()),
            "the lane receives the address as the engine reports it, and nothing else"
        );
    }
}

/// RED (39) — **A page address the machine refuses is said on the surface that was pressed.**
///
/// The address leaves on the lane, so a refusal arrives later; it is said where a refused address
/// is already said on a preview surface (ticket 14's `OnRefused::PreviewAddressRefused`, the
/// surface's toast), and the stderr line names this button, so a page the machine cannot open is
/// not a silent button either.
///
/// MUTATION: answer any other `OnRefused` in `page_address_hand_off` (e.g. `FontsToast`) — the
/// refusal is raised somewhere other than the page that was pressed.
#[test]
fn a_page_address_the_machine_refuses_is_said_on_the_surface_that_was_pressed() {
    let surface = seat_of(TAB_ONE, SeatId(2));
    let address = "http://127.0.0.1:8732/x.html";
    let (_, refused) = page_address_hand_off(surface, address);
    assert_eq!(
        refused,
        handoff_lane::OnRefused::PreviewAddressRefused(surface, address.to_owned())
    );
    let words = PAGE_ADDRESS_REFUSAL.words("no handler");
    assert_eq!(
        words.line.as_deref(),
        Some(
            "recoverable page hand-off failure: open a web page in the system browser: no handler"
        )
    );
    assert_eq!(
        words.notice, None,
        "an address is not a program the reader picked"
    );
}

#[test]
fn local_image_click_routes_preview_external_and_no_effect() {
    let verified = std::path::Path::new(r"C:\tmp\decoded.png");
    assert_eq!(
        local_image_activation(false, true, Some(verified)),
        LocalImageActivation::Preview(verified.to_path_buf()),
        "plain click carries the exact hit path into preview"
    );
    assert_eq!(
        local_image_activation(true, true, Some(verified)),
        LocalImageActivation::External(verified.to_path_buf()),
        "Ctrl+click retains the system-viewer verb"
    );
    assert_eq!(
        local_image_activation(false, true, None),
        LocalImageActivation::None,
        "an unmarked cell has no click side effect"
    );
    assert_eq!(
        local_image_activation(false, false, Some(verified)),
        LocalImageActivation::None,
        "dragging remains selection"
    );
}

/// **P0.** Six hundred turns of the tab strip's ring — ten seconds of a
/// command that prints nothing — must not project the grid once.
///
/// This is the burn the diagnosis measured at 69% of a core: the ring's
/// 16ms deadline held `session.working = true` for a command's whole life,
/// and every tick of it composed a whole terminal picture to redraw a
/// fifteen-pixel arc.
#[test]
fn strip_animation_ticks_project_nothing_while_the_grid_is_still() {
    let mut harness = PtyPresentationHarness::new(80, 24);
    harness.feed_drain(b"$ sleep 30\r\n");
    harness.present_pending();
    let projections_before = harness.viewport_frames;
    let revision_before = harness.content_revision;

    for _ in 0..600 {
        harness.chrome_tick(chrome_tick_reuses_picture);
    }

    assert_eq!(
        harness.viewport_frames - projections_before,
        0,
        "an animation tick must be answered from the picture on the glass"
    );
    assert_eq!(
        harness.content_revision, revision_before,
        "nothing said anything new, so no picture became newer than the glass"
    );
}

/// The other half of the contract: content that *does* change is projected,
/// exactly once, and the ticks around it still cost nothing.
#[test]
fn output_between_animation_ticks_is_projected_exactly_once() {
    let mut harness = PtyPresentationHarness::new(80, 24);
    harness.feed_drain(b"$ sleep 30\r\n");
    harness.present_pending();

    for _ in 0..60 {
        harness.chrome_tick(chrome_tick_reuses_picture);
    }
    let projections_before = harness.viewport_frames;

    harness.feed_drain(b"a line of output\r\n");
    assert_eq!(
        harness.viewport_frames - projections_before,
        1,
        "the shell spoke once and the grid was projected once"
    );
    assert!(
        harness.pending.pending_frame().is_some(),
        "the new picture is composed and waiting for the glass"
    );

    // The tick that lands while it waits presents *that* frame rather than
    // composing a second one on top of it.
    harness.chrome_tick(chrome_tick_reuses_picture);
    assert_eq!(harness.viewport_frames - projections_before, 1);
    assert_eq!(harness.presented_revision, harness.content_revision);

    for _ in 0..60 {
        harness.chrome_tick(chrome_tick_reuses_picture);
    }
    assert_eq!(
        harness.viewport_frames - projections_before,
        1,
        "and the ticks after it are free again"
    );
}

/// **The ring turns at the rate the display declares — no faster.**
///
/// A second of wall clock offered to the loop one millisecond at a time,
/// which is what a window whose every present wakes `about_to_wait` again
/// actually looks like. Sixty-two steps on a 60 Hz panel, not a thousand.
///
/// **And the rate is the window's, not a second one of the strip's own**
/// (closure review 2, 2026-09-18): a 144 Hz panel gets 144 steps, because a
/// window that keeps two rates has a door that admits and a door that
/// refuses at the same instant — which is how a picture that had arrived
/// found every one of them shut.
#[test]
fn the_strip_animation_moves_at_the_rate_of_the_display_it_is_on() {
    let steps_in_a_second = |frame: Duration| {
        let start = Instant::now();
        let mut last = None;
        let mut ticks = 0_u32;
        for tenth in 0..10_000 {
            let now = start + Duration::from_micros(tenth * 100);
            if strip_animation_tick_is_due(last, now, frame) {
                last = Some(now);
                ticks += 1;
            }
        }
        ticks
    };
    // Offered a tenth of a millisecond at a time, so a rate whose period is
    // not a whole number of those can land one step either side of its own
    // arithmetic — 144 Hz is 6.944 ms. The property is the rate, not the
    // rounding.
    let about = |frame: Duration, want: u32| {
        let got = steps_in_a_second(frame);
        assert!(
            got.abs_diff(want) <= 1,
            "one step per {frame:?} is about {want} in a second, not {got}"
        );
    };
    about(pace::DEFAULT_FRAME_INTERVAL, 62);
    about(
        pace::interval_from_millihertz(60_000).expect("60 Hz is a display"),
        60,
    );
    about(
        pace::interval_from_millihertz(144_000).expect("144 Hz is a display"),
        144,
    );
}

/// A window with nothing on the glass yet composes, whatever else is true.
/// There is exactly one such window per lifetime and it is the one whose
/// first picture nobody else is coming to draw.
#[test]
fn the_first_picture_is_never_answered_from_an_empty_screen() {
    assert!(!chrome_tick_reuses_picture(PictureOnGlass {
        frame_pending: false,
        has_presented_frame: false,
        presentation_hold: false,
        content_revision: 0,
        presented_revision: 0,
    }));
}

/// A picture that was composed and never presented is not the picture on
/// the glass, so the tick that finds one waiting presents it instead of
/// building a third.
#[test]
fn a_composed_frame_still_waiting_is_presented_rather_than_recomposed() {
    assert!(chrome_tick_reuses_picture(PictureOnGlass {
        frame_pending: true,
        has_presented_frame: true,
        presentation_hold: false,
        content_revision: 9,
        presented_revision: 8,
    }));
}

/// A held picture is the picture by decision: composing under a hold builds
/// a frame `publish_frame_inner` throws away a few lines later.
#[test]
fn a_presentation_hold_is_answered_from_the_glass_it_is_holding() {
    assert!(chrome_tick_reuses_picture(PictureOnGlass {
        frame_pending: false,
        has_presented_frame: true,
        presentation_hold: true,
        content_revision: 9,
        presented_revision: 4,
    }));
}

/// And the case the revisions exist for: a picture newer than the glass,
/// with nothing queued to carry it there, is composed.
#[test]
fn a_picture_newer_than_the_glass_is_composed() {
    assert!(!chrome_tick_reuses_picture(PictureOnGlass {
        frame_pending: false,
        has_presented_frame: true,
        presentation_hold: false,
        content_revision: 9,
        presented_revision: 8,
    }));
}

/// RED ① — **a present is printed wherever it happens** (§7.10 ④‴).
///
/// The instrument is the reason this defect stood: while
/// `BT_PERF_TRACE present` was printed at the composed door alone, a tab
/// whose gestures were producing **no frames at all** read exactly like a
/// tab whose frames were merely not being printed — and §7.10 ④″ had to
/// write "a tab with no shell draws nothing" down as a known blind spot
/// rather than as the defect it was.
///
/// A tab with no shell only ever takes the retained door
/// (`publish_frame_inner` composes no terminal picture for it), so a line
/// printed at the composed door alone is a line such a tab can never print.
///
/// RED GATE: take `self.trace_present(` out of `present_retained_picture`
/// and the first assertion goes red — which is the build the user was on.
/// Move `last_present_at` below the `trace_perf` gate and the third does:
/// `since_previous_us` is the gap between pictures reaching the glass, and a
/// gap measured against only the presents somebody asked to be told about is
/// not that gap.
#[test]
fn a_present_is_recorded_whichever_door_it_came_through() {
    let retained = squeezed_body("Runtime", "present_retained_picture");
    assert!(
        retained.contains("self.trace_present(trigger.source,receipt,true)"),
        "the retained door is the only door a tab with no shell has, and an \
             instrument that cannot see it says such a tab never draws:\n{retained}"
    );
    let composed = squeezed_body("Runtime", "redraw");
    assert!(
        composed.contains("self.trace_present(trigger.source,receipt,false)"),
        "and the composed door goes on printing what it always printed:\n{composed}"
    );
    let instrument = squeezed_body("Runtime", "trace_present");
    let written = instrument
        .find("self.window.last_present_at=Some(presented_at);")
        .expect("the instrument records when this window last put a picture up");
    let gated = instrument
        .find("if!self.app.trace_perf{return;}")
        .expect("and printing the line is what the flag buys");
    assert!(
        written < gated,
        "the gap between pictures is a fact about the window and not about \
             the trace, so it is measured over every present:\n{instrument}"
    );
}

/// RED ② — **ten notches on a preview alone in a tab are ten presents**
/// (§7.10 ④‴, user report on `next21`).
///
/// The funnel and the door, run against each other the way a wheel runs
/// them. `present_chrome_change` — which 267 call sites in this window
/// reach, the preview's own wheel among them — answers a change that lives
/// in retained renderer state by re-queueing the picture already on the
/// glass and asking for a redraw. **A tab with no shell has no such picture
/// to re-queue, ever**, so all it leaves behind is the bare request, and the
/// rule that answers a bare request is the whole of whether the notch is
/// drawn.
///
/// Measured on the machine at ten notches and zero frames.
///
/// RED GATE: run the same ten notches under the rule as it shipped —
/// `chrome_present_pending` alone — and the count is zero, which is the
/// second half of this test and the defect verbatim.
#[test]
fn ten_notches_on_a_preview_alone_in_a_tab_are_ten_presents() {
    // The wheel reaches the funnel: a notch that moved the document ends in
    // the funnel and in nothing else.
    let wheel = squeezed_body("Runtime", "scroll_preview_body");
    assert!(
        wheel.contains("self.present_chrome_change()"),
        "a preview's wheel asks for its frame through the chrome funnel:\n{wheel}"
    );
    // And the funnel files no debt — it re-queues a picture, and a tab with
    // no shell has none.
    let funnel = squeezed_body("Runtime", "present_chrome_change");
    assert!(
        funnel.contains("letSome(frame)=self.window.last_presented_frame.clone()"),
        "the funnel's whole answer is the picture already on the glass:\n{funnel}"
    );
    assert!(
        !funnel.contains("chrome_present_pending=true"),
        "and it files no debt, which is why the door has to know what kind \
             of tab is asking:\n{funnel}"
    );
    assert!(
        funnel.contains("self.window.window.request_redraw()"),
        "what it always leaves behind is the bare request:\n{funnel}"
    );

    /// The two statements above, as the loop a wheel drives them in.
    ///
    /// `rule` is the question [`Runtime::redraw`] asks when it finds nothing
    /// composed, and it is the only thing that differs between the shipped
    /// build and this one.
    fn notches(count: usize, rule: fn(bool, bool) -> bool) -> usize {
        let tab_has_a_shell = false;
        // A tab with no shell: `activate_tab` empties this on the way in and
        // `publish_frame_inner` composes nothing to refill it with.
        let last_presented_frame: Option<()> = None;
        let mut chrome_present_pending = false;
        let mut slot = None;
        let mut presents = 0;
        for _ in 0..count {
            // `present_chrome_change`.
            if slot.is_none() {
                slot = last_presented_frame;
            }
            let mut redraw_requested = true;
            // `redraw`, at the tail of the same turn.
            while redraw_requested {
                redraw_requested = false;
                match slot.take() {
                    Some(()) => presents += 1,
                    None => {
                        if rule(chrome_present_pending, tab_has_a_shell) {
                            chrome_present_pending = false;
                            presents += 1;
                        }
                    }
                }
            }
        }
        presents
    }

    assert_eq!(
        notches(10, a_bare_redraw_still_owes_a_present),
        10,
        "every notch a person spends on a document has to put that document \
             back on the glass"
    );
    assert_eq!(
        notches(10, |pending, _| pending),
        0,
        "and the narrower question is the shipped build: ten notches, ten \
             scroll offsets, zero frames"
    );
}

/// RED — **a card reads the picture this window already has and asks for
/// nothing** (defect #205; user ruling 2026-08-30; §7.1.6b⁵).
///
/// The asking side of "a card draws the real thumbnail", and the whole of
/// what §7.1.6b′'s red line — *「缩略图不许向磁盘提问」* — costs it. A column
/// of cards walks every seat of every visible tab on every frame; a lookup
/// that armed a decode would be that walk putting a question to the disk for
/// every picture you scrolled past, and the ruling is explicit that the card
/// draws what is already in memory and a face otherwise.
///
/// Four states of one cache and three of them are the same answer, which is
/// the property: `Ready` is the picture, and `Pending`, `Failed` and *absent*
/// are each `None` — with nothing inserted and nothing sent on any of the
/// three.
///
/// RED GATE: give the absent arm the `request_peek_pixels` /
/// `insert(Pending)` pair `file_peek_fitted_pixels` has — which is the
/// obvious way to write it and the way the glance card *does* — and the last
/// assertion goes red with a cache this function was only supposed to read.
#[test]
fn a_card_reads_the_picture_this_window_has_and_asks_for_nothing() {
    let path = Path::new(r"D:\shots\B1-rest.png");
    let key = normalized_local_image_path_key(path);
    let rgba: Arc<[u8]> = Arc::from(vec![0x11; 8 * 2 * 4]);
    let mut cache = PeekCache::with_budget(MAX_PEEK_CACHE_BYTES);

    assert!(
        card_picture_in(&cache, path).is_none(),
        "a file nothing has decoded is a face"
    );
    cache.insert(key.clone(), PeekCacheEntry::Pending);
    assert!(
        card_picture_in(&cache, path).is_none(),
        "a decode that is still out is a face, not a half-drawn picture"
    );
    cache.insert(
        key.clone(),
        PeekCacheEntry::Failed(bt_term::InlineImageDecodeError::UnsupportedFormat),
    );
    assert!(
        card_picture_in(&cache, path).is_none(),
        "and a decode that failed is the same face"
    );
    cache.insert(
        key.clone(),
        PeekCacheEntry::Ready {
            key: key.clone(),
            rgba: Arc::clone(&rgba),
            width_px: 8,
            height_px: 2,
            native_size: None,
        },
    );
    let picture = card_picture_in(&cache, path).expect("a decoded file draws");
    assert_eq!(picture.key, key, "under the decode's own identity");
    assert_eq!((picture.width_px, picture.height_px), (8, 2));
    assert!(
        Arc::ptr_eq(picture.rgba, &rgba),
        "and it is the same pixels the pane is drawing, not a copy"
    );

    // The three refusals left the cache exactly as they found it: this
    // function reads and never asks.
    let reader = free_fn_body("card_picture_in");
    assert!(
        !reader.contains("request_peek_pixels") && !reader.contains(".insert("),
        "the card's picture lookup asks the worker or writes the cache, \
             which is a column of cards reading the disk as you scroll:\n{reader}"
    );
}

/// RED — **and so do this window's animations** (the same row, one map over).
///
/// RED EVIDENCE (2026-09-08), before the budget:
///
/// ```text
/// four animations into a two animation cache: it is holding 8389120
/// ```
///
/// [`WindowRuntime::animations`] was inserted into at two doors and removed
/// from at none, so a folder of spinners hovered one after another kept every
/// one of them decoded until the window closed. The per-animation ceiling
/// [`animation::MAX_ANIMATION_HELD_BYTES`] never applied to the map.
///
/// A small budget rather than the window's own, because the rule is the
/// cache's and the window's number is six rings of frames: what this pins is
/// that an `AnimationEntry` is weighed by the frames it holds, which is the
/// half that lives in this file.
///
/// MUTATION: weigh `AnimationEntry::Ready` as zero and nothing is ever
/// evicted, because nothing is ever counted.
#[test]
fn the_windows_animations_are_bounded_by_the_frames_they_hold() {
    const FRAME_BYTES: usize = 1024 * 1024;
    let frames = || {
        vec![
            animation::AnimationFrame {
                bgra: Arc::from(vec![0x20; FRAME_BYTES]),
                delay: Duration::from_millis(100),
            },
            animation::AnimationFrame {
                bgra: Arc::from(vec![0x40; FRAME_BYTES]),
                delay: Duration::from_millis(100),
            },
        ]
    };
    let mut cache = AnimationCache::with_budget(2 * 2 * FRAME_BYTES as u64);
    for index in 0..4_u8 {
        cache.insert(
            format!(r"d:\spinners\{index}.gif"),
            AnimationEntry::Ready {
                serial: u64::from(index) + 1,
                animation: Box::new(animation::Animation::of(frames(), 512, 512, Instant::now())),
            },
        );
    }
    assert!(
        cache.bytes_held() <= cache.budget(),
        "four animations into a two animation cache: it is holding {}",
        cache.bytes_held(),
    );
    assert!(
        cache.contains_key(r"d:\spinners\3.gif"),
        "the animation asked for last is the one it kept",
    );
    assert!(!cache.contains_key(r"d:\spinners\0.gif"));
    // A refusal weighs nothing but is still remembered, which is what stops a
    // `.gif` this window will not animate being asked about on every frame.
    cache.insert(
        r"d:\spinners\still.gif".to_owned(),
        AnimationEntry::Refused(animation::AnimationRefusal::OneFrame),
    );
    assert!(cache.contains_key(r"d:\spinners\still.gif"));
}

/// **One animated file, held the way the window holds it** — a ring of
/// `frames` frames each standing a tenth of a second, under one playback.
fn a_playback_of(serial: u64, frames: usize, started: Instant) -> AnimationEntry {
    let ring = (0..frames)
        .map(|index| animation::AnimationFrame {
            bgra: Arc::from(vec![index as u8; 4]),
            delay: Duration::from_millis(100),
        })
        .collect();
    AnimationEntry::Ready {
        serial,
        animation: Box::new(animation::Animation::of(ring, 1, 1, started)),
    }
}

/// Which frame of one cached animation is standing.
fn standing_frame_of(cache: &AnimationCache, key: &str) -> u64 {
    match cache.get(key) {
        Some(AnimationEntry::Ready { animation, .. }) => animation.frame_index(),
        _ => panic!("{key} is not a playing animation"),
    }
}

/// What one cached animation's ring is holding.
fn ring_bytes_of(cache: &AnimationCache, key: &str) -> u64 {
    match cache.get(key) {
        Some(AnimationEntry::Ready { animation, .. }) => animation.ring_bytes(),
        _ => panic!("{key} is not a playing animation"),
    }
}

/// The animation fixture, the same file `animation.rs`'s own tests read.
fn an_animated_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/folio-anim-test.gif")
}

/// **One playback of the fixture caught mid-refill** — the animation with
/// its cursor away, and the cursor and frames a worker is bringing home.
///
/// The opening ring is played out first, because an animation whose ring is
/// still full wants nothing and hands out no cursor at all — which is the
/// same sentence that stops a fill being posted twice.
fn a_playback_with_a_fill_in_the_air() -> (
    animation::Animation,
    Box<animation::AnimationCursor>,
    Vec<animation::AnimationFrame>,
) {
    let mut playing = animation::decode(&an_animated_file()).expect("the fixture is four frames");
    let start = Instant::now();
    playing.present(start);
    playing.advance(start + Duration::from_secs(5));
    let (mut cursor, want) = playing
        .take_cursor()
        .expect("a ring that has been played out has room in it");
    assert!(want > 0);
    let frames = cursor.next_frames(want);
    (playing, cursor, frames)
}

/// The upload generation one cached animation is standing on — what the
/// renderer's own gate reads, and therefore the honest answer to "did
/// anything on the glass change".
fn generation_of(cache: &AnimationCache, key: &str) -> u64 {
    match cache.get(key) {
        Some(AnimationEntry::Ready { animation, .. }) => animation.upload().generation,
        _ => panic!("{key} is not a playing animation"),
    }
}

/// RED — **a second animation on one surface is a second picture, not a
/// later frame of the first** (adversarial review 2026-09-11, B3).
///
/// RED EVIDENCE (2026-09-11), the key this window handed the renderer:
///
/// ```text
/// key: format!("gif:{surface:?}")
/// ```
///
/// The renderer holds one texture per key and skips an upload whose
/// generation is not past the one it is already holding (`bt_render`'s
/// `hold_video_texture`) — and **every animation starts its generation at
/// one**. So a pane switched from a `.gif` that had run ten thousand frames
/// to an already-decoded one of the same size kept the first file's pixels
/// and rejected the second's uploads until the second's counter climbed past
/// the first's: at a tenth of a second a frame, a quarter of an hour of the
/// wrong picture under the right name. That is the user's "switching between
/// GIFs stalls", in one line of `format!`.
///
/// A generation only ever means "newer than" *within one playback*, so the
/// playback has to be in the name. The pixel half of the rule — that a
/// renderer handed two names cannot mistake them however the generations
/// compare — is pinned in `bt_render` by
/// `a_second_playback_is_a_second_texture_however_its_frames_are_numbered`.
///
/// MUTATION: drop the serial from `animation_layer_key` and the first
/// assertion fails, which is the defect exactly.
#[test]
fn a_second_playback_on_one_surface_is_a_second_name_for_the_renderer() {
    let card = PreviewSurface::Peek;
    assert_ne!(
        animation_layer_key(card, 1),
        animation_layer_key(card, 2),
        "one box, two files: the renderer must be able to tell them apart",
    );
    // And the same playback keeps its name across frames, which is what
    // makes a texture worth holding at all.
    assert_eq!(animation_layer_key(card, 7), animation_layer_key(card, 7));
    // Two surfaces drawing one playback are still two pictures, because a
    // pane and a card are two boxes at two sizes.
    assert_ne!(
        animation_layer_key(seat_of(TAB_ONE, SeatId(1)), 7),
        animation_layer_key(seat_of(TAB_ONE, SeatId(2)), 7),
    );
    // The window is the third part of the identity and it is held by the
    // renderer's own map rather than spelled in here — see `bt_render`'s
    // `VideoTextureKey`. What has to be true on this side is that the layer
    // list is named through this one function.
    let layers = squeezed_body("Runtime", "video_layers");
    assert!(
        layers.contains("key:animation_layer_key(drawn.surface,drawn.serial),"),
        "the animation layer is named by the one rule:\n{layers}"
    );
}

/// RED — **the clock runs where the picture is drawn, and nowhere else**
/// (adversarial review 2026-09-11, B8).
///
/// RED EVIDENCE (2026-09-11), the two walks that disagreed:
///
/// ```text
/// advance_animations:  for entry in self.window.animations.values_mut()
/// request_animations:  for surface in self.animated_surfaces()
/// ```
///
/// The clock walked **every animated file this window had ever looked
/// inside**; the refills were asked only for the surfaces on the glass. So a
/// `.gif` in a background tab ate the second of frames it had queued, stopped
/// one frame short of its ring with nothing coming to refill it, and — when
/// the tab came back — resumed in the middle of the file and then stood still
/// until a worker round trip returned. Both halves of the user's report are
/// in that pair: the wrong frame, and the pause.
///
/// MUTATION: advance the whole cache again and the second assertion reads
/// 5 instead of 0.
#[test]
fn the_clock_runs_only_where_an_animation_is_drawn() {
    let start = Instant::now();
    let mut cache = AnimationCache::with_budget(MAX_ANIMATION_CACHE_BYTES);
    cache.insert("on the glass".to_owned(), a_playback_of(1, 6, start));
    cache.insert("in another tab".to_owned(), a_playback_of(2, 6, start));
    let drawn: BTreeMap<String, u64> = [("on the glass".to_owned(), 1)].into();
    present_drawn_animations(&mut cache, &BTreeMap::new(), &drawn, start);
    for tick in 1..=10 {
        advance_drawn_animations(&mut cache, &drawn, start + Duration::from_millis(50 * tick));
    }
    assert_eq!(
        standing_frame_of(&cache, "on the glass"),
        5,
        "the animation a reader is looking at plays",
    );
    assert_eq!(
        standing_frame_of(&cache, "in another tab"),
        0,
        "and the one nobody is drawing is a picture holding still",
    );

    // And the refills are asked of the same list, which is the half that
    // makes the first half survivable: a set that advances without being
    // refilled is a ring that drains.
    let asking = squeezed_body("Runtime", "request_animations");
    assert!(
        asking.contains("foranimationindrawn{"),
        "the refill walks the drawn list:\n{asking}"
    );
    assert!(
        asking.contains("ifasked.insert(animation.key.as_str()){"),
        "and asks for each animation once, however many surfaces draw it:\n{asking}"
    );
    let ticking = squeezed_body("Runtime", "advance_animations");
    assert!(
        ticking.contains("&self.window.animations_drawn,"),
        "and the clock reads the record of what was presented:\n{ticking}"
    );
}

/// RED — **a file opened starts at its first frame; a pane revealed
/// resumes** (adversarial review 2026-09-11, B8; the user's "a GIF does not
/// start from its first frame").
///
/// RED EVIDENCE (2026-09-11): there was no record of what a surface was
/// showing at all, so neither sentence could be said. An animation was filed
/// by its file and kept its play head wherever the last surface had left it;
/// picking it in the files column a second time carried on from there, and a
/// tab switch carried on from wherever it had drained to while nobody was
/// drawing it.
///
/// Drawn-ness cannot tell those two apart — a pane in a background tab is
/// not drawn, and neither is one whose file has just changed — which is why
/// the record is about the *subject* of each surface.
///
/// MUTATION: clear the record for a surface that is merely absent from the
/// walk and (3) becomes an open, so every tab switch restarts the animation.
#[test]
fn an_opened_animation_starts_over_and_a_revealed_one_resumes() {
    let pane = seat_of(TAB_ONE, SeatId(1));
    let mut presence: BTreeMap<PreviewSurface, String> = BTreeMap::new();
    let showing = |key: Option<&str>| vec![(pane, key.map(str::to_owned))];

    // (1) the pane is handed a `.gif`: an open.
    assert_eq!(
        animations_opened(&mut presence, &showing(Some("a.gif"))),
        BTreeSet::from(["a.gif".to_owned()]),
    );
    // (2) and on every frame after it, it is the same picture.
    assert!(
        animations_opened(&mut presence, &showing(Some("a.gif"))).is_empty(),
        "a picture that is still there was not opened again",
    );
    // (3) the tab goes away, so the pane is not in the walk at all — and
    // comes back, still about the same file. Not an open: it resumes.
    assert!(animations_opened(&mut presence, &[]).is_empty());
    assert!(
        animations_opened(&mut presence, &showing(Some("a.gif"))).is_empty(),
        "a pane obscured and revealed is not a file opened",
    );
    // (4) the pane is handed another file, and then the first one again.
    // Both are opens, which is what "picked in the files column" means.
    assert_eq!(
        animations_opened(&mut presence, &showing(Some("b.gif"))),
        BTreeSet::from(["b.gif".to_owned()]),
    );
    assert_eq!(
        animations_opened(&mut presence, &showing(Some("a.gif"))),
        BTreeSet::from(["a.gif".to_owned()]),
    );
    // (5) a pane showing something that does not move forgets, so the next
    // `.gif` on it is an open too.
    assert!(animations_opened(&mut presence, &showing(None)).is_empty());
    assert!(presence.is_empty());
    assert_eq!(
        animations_opened(&mut presence, &showing(Some("a.gif"))),
        BTreeSet::from(["a.gif".to_owned()]),
    );

    // And the window lets go of the playback an open replaces, which is what
    // sends the worker to open the file again at frame zero.
    let asking = squeezed_body("Runtime", "request_animations");
    assert!(
        asking.contains("forkeyinanimations_opened(&mutself.window.animation_presence,&named){"),
        "the open is decided by the one rule:\n{asking}"
    );
    assert!(
        asking.contains("self.window.animations.remove(&key);"),
        "and an opened file lets go of the playback it had:\n{asking}"
    );

    // (6) the resume itself, in frames. Hidden on frame two, revealed ten
    // seconds later: it stands on frame two, and stands it out from *now*.
    let start = Instant::now();
    let mut cache = AnimationCache::with_budget(MAX_ANIMATION_CACHE_BYTES);
    cache.insert("a.gif".to_owned(), a_playback_of(1, 6, start));
    let drawn: BTreeMap<String, u64> = [("a.gif".to_owned(), 1)].into();
    let hidden = BTreeMap::new();
    present_drawn_animations(&mut cache, &hidden, &drawn, start);
    advance_drawn_animations(&mut cache, &drawn, start + Duration::from_millis(250));
    assert_eq!(standing_frame_of(&cache, "a.gif"), 2);
    let away = start + Duration::from_secs(10);
    advance_drawn_animations(&mut cache, &hidden, away);
    assert_eq!(
        standing_frame_of(&cache, "a.gif"),
        2,
        "it did not play while nobody was drawing it",
    );
    present_drawn_animations(&mut cache, &hidden, &drawn, away);
    advance_drawn_animations(&mut cache, &drawn, away + Duration::from_millis(99));
    assert_eq!(
        standing_frame_of(&cache, "a.gif"),
        2,
        "and it does not fast-forward through the ten seconds it was away",
    );
    advance_drawn_animations(&mut cache, &drawn, away + Duration::from_millis(100));
    assert_eq!(standing_frame_of(&cache, "a.gif"), 3);
}

/// RED — **a fill answers the playback that asked for it, or nobody**
/// (adversarial review 2026-09-11, B10).
///
/// RED EVIDENCE (2026-09-11), the arrival before this ticket:
///
/// ```text
/// if let Some(AnimationEntry::Ready(mut animation)) = self.window.animations.remove(&key)
/// ```
///
/// The key names the **file**. Since B8 a surface that begins showing a file
/// opens it again, so by the time a cursor comes home the entry under that
/// name may be a second playback standing on frame zero — and parking the old
/// cursor into it hands a fresh animation a decoder halfway through the file
/// and a ring of frames from the middle of it. The picture would jump to
/// wherever the last playback had got to, on the very frame a reader expected
/// it to start.
///
/// The other two ways a fill arrives for nobody are here too. The key
/// evicted: dropped, because re-inserting it would be an eviction undone by
/// its own answer. And the animation no longer drawn: adopted, because the
/// frames are already composed and the ring is bounded, so throwing them away
/// would only buy the decode again — and it **changes nothing a reader or the
/// ceiling can see**, since an undrawn animation neither advances nor asks
/// for more.
///
/// MUTATION: drop the serial from the gate and (1) parks a stranger's
/// decoder, which is the wrong-frame-on-open defect with the sign reversed.
#[test]
fn a_fill_that_answers_a_playback_this_window_has_left_is_dropped() {
    let mut cache = AnimationCache::with_budget(MAX_ANIMATION_CACHE_BYTES);
    // The window's side: one playback that has played out its opening ring,
    // with its cursor away on a worker and frames on the way home. That is
    // the state every refill is posted from.
    let (playing, away, frames) = a_playback_with_a_fill_in_the_air();
    cache.insert(
        "the.gif".to_owned(),
        AnimationEntry::Ready {
            serial: 7,
            animation: Box::new(playing),
        },
    );
    let ring_before = ring_bytes_of(&cache, "the.gif");
    let frame_before = generation_of(&cache, "the.gif");

    // (1) it comes home to a playback this window has moved on from.
    assert_eq!(
        adopt_animation_fill(&mut cache, "the.gif".to_owned(), 6, away, frames),
        AnimationFillOutcome::ForNobody,
        "a fill for a playback that is gone was taken",
    );
    assert_eq!(
        ring_bytes_of(&cache, "the.gif"),
        ring_before,
        "a stranger's frames were parked into this ring",
    );

    // (2) and to a key the map has let go of.
    let (_, cursor, frames) = a_playback_with_a_fill_in_the_air();
    assert_eq!(
        adopt_animation_fill(&mut cache, "gone.gif".to_owned(), 7, cursor, frames),
        AnimationFillOutcome::ForNobody,
        "an eviction was undone by its own answer",
    );

    // (3) and to the playback that actually asked, which is taken.
    let (_, cursor, frames) = a_playback_with_a_fill_in_the_air();
    assert!(!frames.is_empty());
    assert_eq!(
        adopt_animation_fill(&mut cache, "the.gif".to_owned(), 7, cursor, frames),
        AnimationFillOutcome::Parked,
    );
    assert!(
        ring_bytes_of(&cache, "the.gif") > ring_before,
        "the frames it asked for did not land",
    );

    // (4) and a fill that lands for an animation nobody is drawing puts no
    // frame on the glass, however full it leaves the ring.
    let nothing_drawn = BTreeMap::new();
    assert!(
        !advance_drawn_animations(
            &mut cache,
            &nothing_drawn,
            Instant::now() + Duration::from_secs(10)
        ),
        "an undrawn animation moved",
    );
    assert_eq!(
        generation_of(&cache, "the.gif"),
        frame_before,
        "and the renderer is owed no upload either",
    );
}

/// RED — **a `.gif` this window will not play says so where the reader is
/// standing** (user report 2026-09-10).
///
/// RED EVIDENCE (2026-09-10), the second defect in the same report — the
/// reason went to a console:
///
/// ```text
/// eprintln!("BT_GIF {refusal:?} {} (drawn as its first frame)", path.display());
/// ```
///
/// `AnimationEntry::Refused` carried no payload, because all four refusals
/// drew the same picture and the variant had nothing to choose between. That
/// reasoning was right about the picture and wrong about the reader: a
/// window built as a desktop application writes to a `stderr` nobody has
/// open, so a capture that would not play and a spinner with one frame in it
/// were the same silence.
///
/// What is pinned here is the mapping, which is the whole of the rule: the
/// two refusals that leave a reader looking at a picture that ought to be
/// moving get a sentence, and the two that do not are quiet. The strip it
/// lands on is `Runtime::preview_standing_fact`, whose other two sentences are
/// [`preview::PreviewBuffer::read_only_notice`]'s.
///
/// MUTATION: answer `Some` for `OneFrame` and every still `.gif` in a folder
/// wears a notice about not moving.
#[test]
fn an_animation_this_window_will_not_play_says_why_in_the_pane_s_foot() {
    use animation::AnimationRefusal;
    assert_eq!(
        animation_refusal_notice(AnimationRefusal::FrameTooLarge),
        Some(i18n::Text::PreviewAnimationTooLarge.text()),
    );
    assert_eq!(
        animation_refusal_notice(AnimationRefusal::Undecodable),
        Some(i18n::Text::PreviewAnimationBroken.text()),
    );
    // A `.gif` with one frame is a still picture and is drawn as one; a file
    // that is not an animation at all was never this lane's. Neither is news.
    assert_eq!(animation_refusal_notice(AnimationRefusal::OneFrame), None);
    assert_eq!(
        animation_refusal_notice(AnimationRefusal::NotAnAnimation),
        None
    );
    // The two sentences are two sentences, and both are in the shape this
    // strip already speaks in — the state, then the reason.
    let too_large = i18n::Text::PreviewAnimationTooLarge.text();
    let broken = i18n::Text::PreviewAnimationBroken.text();
    assert_ne!(too_large, broken);
    for notice in [too_large, broken] {
        assert!(
            notice.contains('·'),
            "{notice:?} names a state and a reason"
        );
    }
    // And the reason is carried by the entry the foot reads it out of, which
    // is the half of this that used to be missing: an `AnimationEntry` with
    // no payload could not have answered any of the four calls above.
    let entry = AnimationEntry::Refused(AnimationRefusal::FrameTooLarge);
    let AnimationEntry::Refused(carried) = entry else {
        panic!("a refusal is filed with its reason");
    };
    assert_eq!(animation_refusal_notice(carried), Some(too_large));
}

/// RED — **the foot distinguishes a long file from a large frame** (user
/// report 2026-09-12).
///
/// RED EVIDENCE (2026-09-12), the reader's pane, about an 820-pixel-wide
/// recording that had been declined for being 11.7 MB:
///
/// ```text
/// foot: First frame · too large
/// body: Preview failed: inline image exceeds its decode limit
/// ```
///
/// "Its frames are too big to keep two of" and "it is longer than this
/// window will read looking for frames" were one variant wearing one
/// sentence, and the sentence was the first one. They are two facts about
/// two numbers: a two-thousand-square animation is a few megabytes on disk
/// and sixteen decoded, and the reader's file was the opposite of that in
/// both halves. So the refusal splits and the strip says which.
///
/// MUTATION: map both refusals to one sentence and the second assertion
/// goes red, which is the reader being told the wrong thing.
#[test]
fn the_foot_distinguishes_a_long_file_from_a_large_frame() {
    use animation::AnimationRefusal;
    let frame = animation_refusal_notice(AnimationRefusal::FrameTooLarge)
        .expect("a frame too large to stream is worth a sentence");
    let file = animation_refusal_notice(AnimationRefusal::FileTooLong)
        .expect("and so is a file too long to read");
    assert_ne!(frame, file, "two facts about two numbers are two sentences",);
    // Each names the thing it is actually about, in this strip's own shape —
    // the state, then the reason.
    assert_eq!(frame, i18n::Text::PreviewAnimationTooLarge.text());
    assert_eq!(file, i18n::Text::PreviewAnimationFileTooLong.text());
    for notice in [frame, file] {
        assert!(
            notice.contains('·'),
            "{notice:?} names a state and a reason"
        );
    }
    assert!(
        file.contains("file"),
        "{file:?} is about the file's length, and says so"
    );
    // And the third of the three that are worth saying keeps its own words.
    assert_eq!(
        animation_refusal_notice(AnimationRefusal::Undecodable),
        Some(i18n::Text::PreviewAnimationBroken.text()),
    );
}

/// RED — **a file rewritten under the loop ends the playback and is
/// reopened** (user report 2026-09-12, the window's half).
///
/// An animation reads its file rather than holding it, so a file rewritten
/// while the loop is inside it is a decoder about to read the second half of
/// one recording after the first half of another. `AnimationCursor` notices
/// at the moment the loop comes round — see
/// `a_file_rewritten_under_the_loop_ends_the_playback_and_is_reopened` in
/// `animation.rs` — and this is what the window does about it: the playback
/// is let go of, **key and all**, so the pass that opens animations finds no
/// entry under that name and opens the file again. What comes back is the
/// new file, standing on frame zero, with a serial of its own.
///
/// MUTATION: park the stale cursor like any other and the entry stays, which
/// is a pane playing a recording that no longer exists for as long as it is
/// on the glass.
#[test]
fn a_file_rewritten_under_the_loop_ends_the_playback_and_is_reopened() {
    let directory = bt_testpath::temp_path("bt-anim-rewrite");
    std::fs::create_dir_all(&directory).expect("a directory this test owns");
    let path = directory.join("capture.gif");
    std::fs::copy(an_animated_file(), &path).expect("the fixture, under a name this test owns");

    let mut cache = AnimationCache::with_budget(MAX_ANIMATION_CACHE_BYTES);
    let mut playing = animation::decode(&path).expect("the fixture is four frames");
    let start = Instant::now();
    playing.present(start);
    playing.advance(start + Duration::from_secs(5));
    let (mut cursor, want) = playing.take_cursor().expect("a played-out ring has room");
    cache.insert(
        "capture.gif".to_owned(),
        AnimationEntry::Ready {
            serial: 11,
            animation: Box::new(playing),
        },
    );

    // The file is rewritten while the cursor is out — a longer one, which is
    // what an export written again is.
    let mut longer = std::fs::read(an_animated_file()).expect("the fixture reads");
    longer.extend_from_slice(&std::fs::read(an_animated_file()).expect("twice"));
    std::fs::write(&path, &longer).expect("the file is written again");
    let frames = cursor.next_frames(want.max(16));

    assert_eq!(
        adopt_animation_fill(&mut cache, "capture.gif".to_owned(), 11, cursor, frames),
        AnimationFillOutcome::FileChanged,
        "a cursor that came round onto another file was parked as if nothing had happened",
    );
    assert!(
        !cache.contains_key("capture.gif"),
        "the playback is let go of, so the next pass opens the file again",
    );

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(&directory);
}

/// RED — **a recording follows the pane it is drawn in** (user report on
/// `next22`, defects #202/#204; §7.44 ⑮).
///
/// `a_page_follows_the_pane_it_is_drawn_in`'s sentence said about the second
/// window-level table a pane can be behind, and it is the same defect with a
/// different ending. [`WindowRuntime::video`] is keyed by
/// [`PreviewSurface`], both halves of a seat surface's name change on a
/// move, and [`Runtime::sweep_video_seats`] retires every seat whose surface
/// has stopped existing — so a video pane dragged into another tab lost its
/// decoder on the very next frame while its head, its control bar and its
/// fact line travelled with the pane.
///
/// The three things stated here are the three the fix is:
///
/// ① **Only a seat travels.** A float and the glance card are not leaves,
/// so a move cannot be about them, and a build that keyed the carrier by
/// leaf alone would have rehomed a card's recording onto a pane.
/// ② **The doors.** The same three the page carrier is called from, because
/// they are the three gestures that re-key a pane inside one window.
/// ③ **The transaction.** Every travelling recording off its surface before
/// any of them is put down — a trade files the arriving one under the
/// departing one's key, and `put` shuts down whatever it lands on.
///
/// RED GATE: take the call out of any one of the three doors and that door's
/// pin goes red; run the carrier as a loop of `rehome` instead of
/// `take`-then-`put` and the `.collect();` between the two disappears, which
/// on a trade is a live engine shut down.
#[test]
fn a_recording_follows_the_pane_it_is_drawn_in() {
    // ① a seat is a leaf and the other two surfaces are not, which is what
    // makes "which of these moves is about a recording" answerable at all.
    let leaf = LeafId {
        tab: TabId(1),
        seat: SeatId(2),
    };
    let surfaces = [
        PreviewSurface::Seat(leaf),
        PreviewSurface::Float(9),
        PreviewSurface::Peek,
    ];
    let leaves: Vec<LeafId> = surfaces
        .iter()
        .filter_map(|surface| match surface {
            PreviewSurface::Seat(leaf) => Some(*leaf),
            PreviewSurface::Float(_) | PreviewSurface::Peek => None,
        })
        .collect();
    assert_eq!(
        leaves,
        vec![leaf],
        "a float and a glance card have no address a move could change"
    );

    // ② the three doors.
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
            text.contains("self.carry_the_recordings_of_moved_panes("),
            "{door} leaves its recording behind, and a recording left behind \
                 is swept away a frame later:\n{text}"
        );
    }

    // ③ the transaction.
    let carrier = squeezed_body("Runtime", "carry_the_recordings_of_moved_panes");
    let removed = carrier
        .find("self.window.video.take(")
        .expect("the recordings are lifted off their surfaces");
    let collected = removed
        + carrier[removed..]
            .find(".collect();")
            .expect("and gathered before any of them is put down");
    let put = carrier
        .find("self.window.video.put(now,seat);")
        .expect("and then put down under their new names");
    assert!(
        removed < collected && collected < put,
        "a trade files the arriving recording under the departing one's own \
             surface, so a put interleaved with the takes shuts down a live \
             decoder:\n{carrier}"
    );
}

/// RED: an obsolete completion must not clear the newest pending target, and the newest
/// completion must retire "Loading image..." instead of leaving the preview stuck forever.
#[test]
fn preview_loading_survives_a_stale_scale_answer_then_clears_on_the_latest() {
    let mut preview = PreviewImageState::new(PathBuf::from("storm.png"));
    preview.pending = Some(("same-path".to_owned(), 320, 180));

    assert!(!preview.accept_scaled(bt_term::scale_inline_image(&scale_task("same-path", 160,))));
    assert_eq!(preview.message(), Some("Loading storm.png…".to_owned()));

    assert!(preview.accept_scaled(bt_term::scale_inline_image(
        &bt_term::InlineImageScaleTask {
            display_width_px: 320,
            display_height_px: 180,
            ..scale_task("same-path", 320)
        },
    )));
    assert_eq!(preview.message(), None);
}

// ── the picture keeps its identity across a change of host ─────────────
//
// User report, 2026-08-17: "undock an image preview into a float and the
// picture disappears; dock it back and it still never loads". Three pins,
// one per link of the chain that broke.

fn pending_picture(path: &str, target: PeekThumbnailTarget) -> PreviewImageState {
    let mut picture = PreviewImageState::new(PathBuf::from(path));
    picture.pending = Some(target);
    picture
}

/// RED before the fix: the resample was delivered to whoever held the
/// texture lane *when it landed*, and `pop_out_preview` hands the lane back
/// on the way out — so a question asked by a docked pane and answered while
/// it was a window went to nobody, and the ledger entry it should have
/// retired stayed set for good ("already asked" for every later refit).
///
/// The move here is exactly `pop_out_preview`'s: the pane is taken out of the
/// plane whole and put back under a float's address.
#[test]
fn a_picture_torn_off_into_a_window_still_takes_delivery_of_its_resample() {
    let seat = SeatId(7);
    let float = 3_u64;
    let target: PeekThumbnailTarget = ("push-pin".to_owned(), 320, 180);
    let mut panes = PreviewPanes::default();
    panes.entry(seat_of(TAB_ONE, seat)).image =
        Some(pending_picture("push-pin.png", target.clone()));

    assert_eq!(
        panes.awaiting_scale(&target),
        Some(seat_of(TAB_ONE, seat)),
        "the docked pane is the one that asked"
    );

    // `pop_out_preview`: the view travels whole, under a new address.
    let pane = panes.remove(seat_of(TAB_ONE, seat)).expect("the pane");
    *panes.entry(PreviewSurface::Float(float)) = pane;

    assert_eq!(
        panes.awaiting_scale(&target),
        Some(PreviewSurface::Float(float)),
        "and the window it became is still the one that asked"
    );
    // The texture lane, meanwhile, is nobody's — `pop_out_preview` hands it
    // back on the way out, because a window paints its own picture. That is
    // exactly why the answer must not be routed through it: at this instant
    // there is no lane holder to hand it to, and the question above would
    // then stay outstanding for good.
    let lane: Option<PreviewSurface> = None;
    assert_ne!(
        lane,
        panes.awaiting_scale(&target),
        "the picture that asked is not the pane that happens to hold the lane"
    );

    let surface = panes.awaiting_scale(&target).expect("an asker");
    let picture = panes
        .entry(surface)
        .image
        .as_mut()
        .expect("the picture that asked");
    assert!(
        picture.accept_scaled(bt_term::scale_inline_image(
            &bt_term::InlineImageScaleTask {
                display_width_px: 320,
                display_height_px: 180,
                ..scale_task("push-pin", 320)
            },
        )),
        "the answer lands on the picture that asked for it"
    );
    assert_eq!(picture.pending, None, "and retires the question with it");
    assert!(picture.raster.is_some(), "leaving the pixels behind");
    assert_eq!(
        panes.awaiting_scale(&target),
        None,
        "nothing is left waiting on an answer that has arrived"
    );
}

/// The other half of the ledger's contract: an outstanding question belongs
/// to a *surface*, so it goes away with it. A pending target that outlived
/// its pane would be an answer routed to a host that no longer exists — and,
/// worse, a target a later picture could match by accident.
#[test]
fn an_in_flight_resample_cannot_outlive_the_surface_that_asked_for_it() {
    let seat = seat_of(TAB_ONE, SeatId(9));
    let target: PeekThumbnailTarget = ("push-pin".to_owned(), 64, 64);
    let mut panes = PreviewPanes::default();
    panes.entry(seat).image = Some(pending_picture("push-pin.png", target.clone()));
    assert_eq!(panes.awaiting_scale(&target), Some(seat));

    panes.remove(seat);
    assert_eq!(
        panes.awaiting_scale(&target),
        None,
        "the pane closed and took its question with it"
    );

    // And the same when the surface stays but stops showing a picture.
    panes.entry(seat).image = Some(pending_picture("push-pin.png", target.clone()));
    panes.entry(seat).image = None;
    assert_eq!(
        panes.awaiting_scale(&target),
        None,
        "a surface that gave up its picture is not waiting on one"
    );
}

#[test]
fn preview_resize_storm_reuses_the_shared_quiet_boundary() {
    let start = Instant::now();
    let last = start + Duration::from_millis(90);
    let mut preview = PreviewImageState::new(PathBuf::from("storm.png"));
    preview.defer_scale_settle(start);
    preview.defer_scale_settle(start + Duration::from_millis(40));
    preview.defer_scale_settle(last);

    assert!(
        !preview
            .finish_scale_settle_if_quiet(last + WINDOW_RESIZE_QUIET - Duration::from_millis(1))
    );
    assert!(preview.finish_scale_settle_if_quiet(last + WINDOW_RESIZE_QUIET));
    assert_eq!(preview.scale_settle_deadline, None);
    assert!(!preview.finish_scale_settle_if_quiet(last + WINDOW_RESIZE_QUIET));
}

/// **A zoom is a settle and a pan is not**, which is what keeps a drag off
/// the resample lane and a wheel on it.
///
/// Measured before this was wired up (4000×3000 PNG, ten detents): seven
/// exact-size questions went to the lane, it had time to answer three, and
/// two of the three were sizes the gesture had already travelled past — so
/// the raster on screen jumped twice on its way to the one that was asked
/// for. Deferring the question to the boundary the resize gesture already
/// keeps leaves exactly one pass, at the size the hand stopped on.
#[test]
fn a_scale_starts_the_settle_again_and_a_pan_never_does() {
    let fit = ImageZoom::FIT;
    let bigger = ImageZoom::scaled(1.25);
    assert!(
        image_zoom_settles(fit, bigger),
        "a wheel that changed the scale owes a new exact-size raster"
    );
    assert!(
        image_zoom_settles(bigger, fit),
        "and so does one that gave it back"
    );

    let carried = ImageZoom {
        pan: [40.0, -18.0],
        ..bigger
    };
    assert!(
        !image_zoom_settles(bigger, carried),
        "a drag moves the picture across a raster it already holds, so the \
             lane is asked nothing"
    );
    assert!(
        !image_zoom_settles(
            carried,
            ImageZoom {
                pan: [-7.0, 3.0],
                ..bigger
            }
        ),
        "and the next move of the same drag asks it nothing either"
    );
}

// ── ticket #60: the picture zooms and pans ──────────────────────────────

/// A 1000×500 body is a convenient one to read the fractions off: the image
/// column is 860 × 350, so a 2000×1000 picture fits to 350 tall and a
/// 100×50 one is drawn at its own size rather than blown up to fill it.
const ZOOM_BODY: [f32; 4] = [100.0, 200.0, 1100.0, 700.0];

fn rect_size(rect: [f32; 4]) -> (f32, f32) {
    (rect[2] - rect[0], rect[3] - rect[1])
}

#[test]
fn fit_contains_the_picture_in_the_image_column_and_never_enlarges_it() {
    let fitted = image_destination(ZOOM_BODY, [2000, 1000], ImageZoom::FIT);
    let (width, height) = rect_size(fitted);
    assert_close(height, 350.0, "height fills the column's 70%");
    assert_close(width, 700.0, "and the aspect ratio decides the width");
    assert_close(
        (fitted[0] + fitted[2]) / 2.0,
        600.0,
        "centred on the whole body, not on the column",
    );
    assert_close((fitted[1] + fitted[3]) / 2.0, 450.0, "vertically too");

    let small = image_destination(ZOOM_BODY, [100, 50], ImageZoom::FIT);
    assert_eq!(
        rect_size(small),
        (100.0, 50.0),
        "a small picture is drawn at its own size; Fit contains, it does not stretch"
    );
}

/// RED — **a video's still lands exactly where the playing picture does**
/// (user ruling 2026-08-28; `docs/DESIGN.md` §7.42).
///
/// The app-side half of `the_still_and_the_playback_share_one_fit_rule`.
/// [`video_still_destination`] is what a paused pane's frame is drawn by and
/// [`bt_render::video_frame_rect`] is what the renderer draws the moving
/// picture by; if the two could give different rectangles, pressing play
/// would move the picture, which is the thing a reader would notice first
/// and be least able to describe.
///
/// The 160×120 case is the recording in this repository's `tests/assets` and
/// the one the defect was reported on: the still fills the body, where
/// [`image_destination`] would draw it at 160×120 in the middle of it.
///
/// RED GATE: put `image_destination` back in `refit_preview_picture`'s video
/// arm — which is the state that shipped in `next12` — and the last block
/// here names the size it draws instead.
#[test]
fn a_videos_still_lands_where_the_playing_picture_does() {
    for video in [[160_u32, 120_u32], [1920, 1080], [1080, 1920], [640, 640]] {
        let still = video_still_destination(ZOOM_BODY, video);
        let box_ = bt_render::SeatViewport {
            x: ZOOM_BODY[0] as u32,
            y: ZOOM_BODY[1] as u32,
            width: (ZOOM_BODY[2] - ZOOM_BODY[0]) as u32,
            height: (ZOOM_BODY[3] - ZOOM_BODY[1]) as u32,
        };
        let playing = bt_render::video_frame_rect(box_, video[0], video[1]).expect("a rectangle");
        assert_close(
            rect_size(still).0,
            rect_size(playing).0,
            "the still and the playing picture are one width",
        );
        assert_close(rect_size(still).1, rect_size(playing).1, "and one height");
        // **The same rectangle and not merely the same size** (2026-08-28).
        // The origins are compared too, because half a pixel of
        // disagreement between them is exactly the flicker pressing play
        // would show — and half a pixel is what the two of them were apart
        // while the still centred itself on a floating-point midpoint and
        // the layer split its leftover by a floor.
        for axis in 0..4 {
            assert_close(still[axis], playing[axis], "one rectangle, not two");
        }
        // And it sits on the body's centre as closely as a pixel grid
        // allows. Half a pixel and not zero: `video_frame_rect` splits the
        // leftover by a floor, so an odd leftover is a pixel of ground on
        // one edge and none on the other — which is what every other
        // centred thing in this window does and is invisible either way.
        for (centre, want) in [
            ((still[0] + still[2]) / 2.0, 600.0_f32),
            ((still[1] + still[3]) / 2.0, 450.0),
        ] {
            assert!(
                (centre - want).abs() <= 0.5,
                "centred: {centre} is more than half a pixel off {want}"
            );
        }
    }
    // And the rule it is *not*: the picture channel would leave the
    // repository's own fixture at its own 160×120 in a 1000×500 body.
    let (width, height) = rect_size(video_still_destination(ZOOM_BODY, [160, 120]));
    assert_close(height, 500.0, "a video fills the body it is given");
    assert_close(width, 667.0, "at its own proportion");
    assert_eq!(
        rect_size(image_destination(ZOOM_BODY, [160, 120], ImageZoom::FIT)),
        (160.0, 120.0),
        "which is not what the picture channel does, and still should not be"
    );
}

#[test]
fn a_hundred_percent_draws_one_image_pixel_per_screen_pixel_and_centres_it() {
    let drawn = image_destination(ZOOM_BODY, [400, 300], ImageZoom::scaled(1.0));
    assert_eq!(rect_size(drawn), (400.0, 300.0));
    assert_close((drawn[0] + drawn[2]) / 2.0, 600.0, "centred");
    assert_close((drawn[1] + drawn[3]) / 2.0, 450.0, "centred");
    assert_eq!(
        image_destination(ZOOM_BODY, [400, 300], ImageZoom::FIT),
        drawn,
        "and this picture's Fit happens to be 100% too, because Fit never enlarges"
    );
}

#[test]
fn a_zoomed_picture_may_be_carried_only_until_its_own_edge_reaches_the_bodys() {
    let image = [400_u32, 300];
    // 250% of 400×300 is 1000×750: one axis exactly the body's width, the
    // other 250px taller than its 500.
    let centred = image_destination(ZOOM_BODY, image, ImageZoom::scaled(2.5));
    assert_eq!(rect_size(centred), (1000.0, 750.0));

    let carried = image_destination(
        ZOOM_BODY,
        image,
        ImageZoom {
            mode: ImageZoomMode::Scale(2.5),
            pan: [40.0, 60.0],
        },
    );
    assert_close(
        carried[0],
        centred[0],
        "an axis with no overflow does not move at all",
    );
    assert_close(carried[1], centred[1] + 60.0, "the other one does");

    let overrun = image_destination(
        ZOOM_BODY,
        image,
        ImageZoom {
            mode: ImageZoomMode::Scale(2.5),
            pan: [0.0, 9000.0],
        },
    );
    assert_close(
        overrun[1],
        ZOOM_BODY[1],
        "and stops when its own top edge reaches the body's, never opening a gap",
    );
    assert_close(overrun[3], ZOOM_BODY[1] + 750.0, "the far edge follows it");

    let under = image_destination(
        ZOOM_BODY,
        image,
        ImageZoom {
            mode: ImageZoomMode::Scale(0.5),
            pan: [500.0, 500.0],
        },
    );
    assert_eq!(
        under,
        image_destination(ZOOM_BODY, image, ImageZoom::scaled(0.5)),
        "a picture smaller than the body is centred whatever the pan says"
    );
}

#[test]
fn the_scale_is_clamped_to_a_tenth_and_eight_times_the_native_pixels() {
    let image = [400_u32, 300];
    assert_eq!(
        rect_size(image_destination(
            ZOOM_BODY,
            image,
            ImageZoom::scaled(1000.0)
        )),
        (400.0 * IMAGE_ZOOM_MAX, 300.0 * IMAGE_ZOOM_MAX)
    );
    assert_eq!(
        rect_size(image_destination(ZOOM_BODY, image, ImageZoom::scaled(0.0))),
        (400.0 * IMAGE_ZOOM_MIN, 300.0 * IMAGE_ZOOM_MIN)
    );
}

/// **A picture bigger than its body on both axes is still somewhere on
/// it** — the invariant the 2026-09-10 report was read against.
///
/// A 2720×3000 picture at 123% in a body a third its size overruns the body
/// on every side, and that is the *ordinary* look of a zoomed picture, not
/// an edge case: the rectangle is real, it covers the body outright, and the
/// pan has room on both axes. The report's blank pane was not this
/// arithmetic — it was the shared texture cache dropping the raster between
/// the frame resolving it and the pass issuing it (`bt-render`'s
/// `CachedMathTexture`) — and this stands so that a future clamp cannot
/// quietly answer the same report by shrinking the rectangle instead.
#[test]
fn a_picture_larger_than_its_body_on_both_axes_still_covers_it() {
    // The reader's own file and the reader's own pane, in physical pixels.
    let image = [2720_u32, 3000];
    let body = [200.0_f32, 300.0, 1475.0, 2028.0];
    let zoom = ImageZoom::scaled(1.23);
    let rect = image_destination(body, image, zoom);
    let (width, height) = rect_size(rect);
    assert!(
        width > body[2] - body[0] && height > body[3] - body[1],
        "the drawn picture really is larger than the body on both axes"
    );
    assert!(
        rect[0] < body[2] && rect[2] > body[0] && rect[1] < body[3] && rect[3] > body[1],
        "and it is drawn across the body rather than anywhere else"
    );
    assert!(
        rect[0] <= body[0] && rect[1] <= body[1] && rect[2] >= body[2] && rect[3] >= body[3],
        "a picture this size leaves no ground showing on any side"
    );
}

/// **A picture taller than its body is carried up and down, on the same
/// terms it is carried left and right** (user report, 2026-09-10).
///
/// One rule per axis and the same rule — [`clamp_image_pan`] is handed the
/// body's own extent on the axis it is asked about, not the fraction
/// [`image_fit_scale`] reserves for the meta line — so the travel each axis
/// gets is half its own overflow and nothing else. Written down because the
/// report said the vertical half did not move, and the only way to keep that
/// true is to state what "moves" means.
#[test]
fn a_picture_taller_than_its_body_is_carried_up_and_down() {
    let image = [2720_u32, 3000];
    let body = [200.0_f32, 300.0, 1475.0, 2028.0];
    let zoom = ImageZoom::scaled(1.23);
    let rested = image_destination(body, image, zoom);
    let (width, height) = rect_size(rested);
    let (across, down) = (
        (width - (body[2] - body[0])) / 2.0,
        (height - (body[3] - body[1])) / 2.0,
    );
    assert!(across > 0.0 && down > 0.0, "there is road on both axes");

    let carried = |pan: [f32; 2]| image_destination(body, image, ImageZoom { pan, ..zoom });
    assert_close(
        carried([0.0, -300.0])[1],
        rested[1] - 300.0,
        "three hundred pixels up is three hundred pixels up",
    );
    assert_close(
        carried([-300.0, 0.0])[0],
        rested[0] - 300.0,
        "and the same hand across gets the same distance",
    );
    // And each axis stops at its own end of the road, never at the other's:
    // the edge that comes to rest is the one the hand was travelling
    // towards, and no ground is opened behind it.
    assert_close(
        carried([0.0, -down * 4.0])[3],
        body[3],
        "carried up past the end, the picture's bottom edge rests on the body's",
    );
    assert_close(
        carried([0.0, down * 4.0])[1],
        body[1],
        "and carried down past the end, its top edge rests on the body's top",
    );
    assert_close(
        carried([across * 4.0, 0.0])[0],
        body[0],
        "the same sentence sideways: carried right past the end, its left \
             edge rests on the body's left",
    );
}

#[test]
fn a_wheel_notch_over_the_picture_zooms_about_the_pointer() {
    let image = [400_u32, 300];
    // Fit for this picture is 100%, so the first notch out of Fit is 125%.
    let pointer = [900.0, 300.0];
    let one = image_zoom_notch(ImageZoom::FIT, 1.0, ZOOM_BODY, image, pointer);
    assert_eq!(one.mode, ImageZoomMode::Scale(IMAGE_ZOOM_STEP));

    // The anchor is asked of a picture that is *already* larger than the
    // body on both axes, because that is the only state in which an anchor
    // can be honoured at all: on an axis with no overflow the clamp centres
    // the picture, and centring is a stronger promise than the pointer's.
    let held = ImageZoom::scaled(3.0);
    let before = image_destination(ZOOM_BODY, image, held);
    let zoomed = image_zoom_notch(held, 1.0, ZOOM_BODY, image, pointer);
    let after = image_destination(ZOOM_BODY, image, zoomed);
    for axis in 0..2 {
        // Where the pointer sat inside the picture, as a fraction of it.
        let was = (pointer[axis] - before[axis]) / (before[axis + 2] - before[axis]);
        let now = (pointer[axis] - after[axis]) / (after[axis + 2] - after[axis]);
        assert_close(now, was, "the same point of the picture is under the hand");
    }

    let down = image_zoom_notch(one, -1.0, ZOOM_BODY, image, pointer);
    assert_eq!(
        down.mode,
        ImageZoomMode::Scale(1.0),
        "and the notch is multiplicative, so one back is exactly where it started"
    );

    assert_eq!(
        image_zoom_notch(
            ImageZoom::scaled(IMAGE_ZOOM_MAX),
            1.0,
            ZOOM_BODY,
            image,
            pointer
        ),
        ImageZoom::scaled(IMAGE_ZOOM_MAX),
        "a notch at the end of the road is spent on nothing and moves nothing"
    );
}

/// **A detent delivered in pieces is worth exactly one detent** — the claim
/// `docs/handoff/HANDOFF-2026-08-21.md` §5 ⑮/⑳ made against this gesture,
/// checked rather than inherited.
///
/// That report reasoned by analogy from the focus card's aim, which shares
/// [`wheel_zoom_notches`] and *did* lose the remainder: the aim spends whole
/// rows, so it takes `trunc` and had to grow [`CardAim`] to carry the
/// fraction over. The picture's zoom has no such floor. It is continuous in
/// the notch count — `IMAGE_ZOOM_STEP.powf(notches)` — and `s^a · s^b =
/// s^(a+b)`, so the six twenty-unit reports a precision touchpad sends per
/// detent compose into the same 125% the one report of a mouse gets, with
/// nothing carried and nothing dropped.
///
/// Confirmed on the machine as well as here (`ui-probe wheel -Step 20`, six
/// reports): 0.9832 → 1.0204 → 1.0591 → 1.0992 → 1.1409 → 1.1841 → 1.2290,
/// which is 0.9832 × 1.25 to four figures, and every report moved the
/// picture rather than six of them moving it once.
#[test]
fn a_detent_delivered_in_pieces_zooms_exactly_as_one_detent() {
    let image = [400_u32, 300];
    let pointer = [900.0, 300.0];
    let whole = image_zoom_notch(ImageZoom::FIT, 1.0, ZOOM_BODY, image, pointer);

    // What Win32 sends when a high-resolution wheel or a precision touchpad
    // turns one detent: six reports of twenty, not one of a hundred and
    // twenty.
    let piece = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 20.0));
    let mut stepped = ImageZoom::FIT;
    for report in 0..6 {
        let moved = image_zoom_notch(
            stepped,
            wheel_zoom_notches(piece),
            ZOOM_BODY,
            image,
            pointer,
        );
        assert_ne!(
            moved, stepped,
            "report {report} of the detent moved the picture by nothing"
        );
        stepped = moved;
    }

    let (ImageZoomMode::Scale(whole), ImageZoomMode::Scale(stepped)) = (whole.mode, stepped.mode)
    else {
        panic!("a notch out of Fit resolves to a scale");
    };
    assert_close(
        stepped,
        whole,
        "six pieces of a detent are the detent, with no remainder thrown away",
    );
}

#[test]
fn a_double_click_swings_between_fit_and_a_hundred_percent_and_lands_centred() {
    let hundred = image_zoom_toggled(ImageZoom::FIT);
    assert_eq!(hundred.mode, ImageZoomMode::Scale(1.0));
    assert_eq!(
        hundred.pan,
        [0.0, 0.0],
        "a toggle is a command and lands on the middle, not on wherever the second click fell"
    );
    assert_eq!(image_zoom_toggled(hundred), ImageZoom::FIT);
    assert_eq!(
        image_zoom_toggled(ImageZoom {
            mode: ImageZoomMode::Scale(3.0),
            pan: [90.0, 4.0],
        }),
        ImageZoom::FIT,
        "and from anywhere else it goes home"
    );
}

/// RED (42) — **a press on a float's body climbs the docked pane's ladder, picture
/// included.**
///
/// A zoomed picture in a floating window could be neither panned nor
/// double-clicked (owner, 2026-09-23: 「悬浮窗中的图片无法拖动」), while the same
/// picture docked did both. `press_float` claims every press inside a window
/// above the chrome router and restated the docked pane's body ladder for
/// itself, and the restatement left out `press_preview_image` — the one rung
/// that arms the pan and counts the double click. The move and release halves
/// were never missing. The ladder is now one value, [`PreviewBodyRung::LADDER`],
/// walked by one function that both hosts call, so the picture's rung is the
/// float's because it is the pane's.
///
/// A `Runtime` needs a window and a GPU, so the routing is read off the
/// bodies (through `bt_source`) and the order off the value itself.
///
/// MUTATION: remove the picture's rung from `PreviewBodyRung::LADDER` (the
/// call a float's body press makes into `press_preview_image`) — the order
/// assertion goes red; restate the list in `press_float` again and the reader
/// assertions go red.
#[test]
fn a_press_on_a_floats_picture_climbs_the_docked_panes_ladder() {
    assert_eq!(
        PreviewBodyRung::LADDER.as_slice(),
        [
            PreviewBodyRung::BodyThumb,
            PreviewBodyRung::BlockThumb,
            PreviewBodyRung::Picture,
            PreviewBodyRung::EditSurface,
            PreviewBodyRung::RenderedText,
        ]
        .as_slice(),
        "the furniture, then the picture, then the edit surface and the page"
    );
    assert_eq!(
        reader_names(&calls_of("Runtime", "press_preview_body_ladder")),
        ["chrome_mouse_input", "press_float"],
        "the docked pane and the float walk one ladder"
    );
    assert_eq!(
        reader_names(&calls_of("Runtime", "press_preview_image")),
        ["press_preview_body_ladder"],
        "the picture's rung is asked from one place"
    );
    // Neither host asks a rung for itself: that would be a second list. (The
    // glance card asks the block bar for its own body, which no preview surface
    // walk can see — `press_file_peek` is not one of the two hosts.)
    for rung in [
        "press_preview_body_thumb",
        "press_preview_block_thumb",
        "press_preview_body",
        "press_preview_text",
    ] {
        let readers = reader_names(&calls_of("Runtime", rung));
        assert!(
            readers
                .iter()
                .any(|name| name == "press_preview_body_ladder")
                && !readers
                    .iter()
                    .any(|name| name == "press_float" || name == "chrome_mouse_input"),
            "`{rung}` is asked outside the ladder by a host: {readers:?}"
        );
    }
}

/// PIN (42) — **a double click on a float's picture is a double click, and it
/// toggles the zoom.**
///
/// The half of `press_preview_image` a float's press now reaches: the float is a
/// surface that takes zoom, two presses in one place on it are a pair, and the
/// pair swings the picture between fit and a hundred percent. Green before the
/// fix too — the gap was the routing above, not this arithmetic.
#[test]
fn a_double_click_on_a_floats_picture_toggles_its_zoom() {
    let surface = PreviewSurface::Float(9);
    assert!(surface_takes_image_zoom(surface));
    let now = Instant::now();
    let mut clicks = ImageClicks::default();
    assert!(!clicks.register(surface, [400.0, 300.0], now));
    assert!(clicks.register(surface, [402.0, 301.0], now + Duration::from_millis(90)));
    assert_eq!(
        image_zoom_toggled(ImageZoom::FIT).mode,
        ImageZoomMode::Scale(1.0)
    );
}

#[test]
fn two_presses_in_one_place_are_a_double_click_and_a_third_starts_over() {
    let surface = seat_of(TAB_ONE, SeatId(2));
    let now = Instant::now();
    let mut clicks = ImageClicks::default();
    assert!(!clicks.register(surface, [400.0, 300.0], now));
    assert!(clicks.register(surface, [403.0, 302.0], now + Duration::from_millis(90)));
    assert!(
        !clicks.register(surface, [403.0, 302.0], now + Duration::from_millis(120)),
        "the pair was spent; a third press begins a new one"
    );

    let mut slow = ImageClicks::default();
    assert!(!slow.register(surface, [400.0, 300.0], now));
    assert!(!slow.register(
        surface,
        [400.0, 300.0],
        now + MULTI_CLICK_INTERVAL + Duration::from_millis(1)
    ));

    let mut wandered = ImageClicks::default();
    assert!(!wandered.register(surface, [400.0, 300.0], now));
    assert!(!wandered.register(surface, [480.0, 300.0], now + Duration::from_millis(90)));

    let mut elsewhere = ImageClicks::default();
    assert!(!elsewhere.register(surface, [400.0, 300.0], now));
    assert!(!elsewhere.register(
        seat_of(TAB_ONE, SeatId(3)),
        [400.0, 300.0],
        now + Duration::from_millis(90)
    ));
}

#[test]
fn the_four_zoom_keys_step_about_the_body_centre_and_name_the_two_ends() {
    let image = [400_u32, 300];
    let key = |text: &str| Key::Character(text.into());
    assert_eq!(
        image_zoom_key(ImageZoom::FIT, &key("+"), ZOOM_BODY, image),
        Some(ImageZoom::scaled(IMAGE_ZOOM_STEP)),
        "a key has no pointer, so its anchor is the centre and the pan stays nothing"
    );
    assert_eq!(
        image_zoom_key(ImageZoom::FIT, &key("="), ZOOM_BODY, image),
        image_zoom_key(ImageZoom::FIT, &key("+"), ZOOM_BODY, image),
        "`=` is the unshifted `+` and means the same thing"
    );
    assert_eq!(
        image_zoom_key(ImageZoom::scaled(1.0), &key("-"), ZOOM_BODY, image),
        Some(ImageZoom::scaled(1.0 / IMAGE_ZOOM_STEP))
    );
    assert_eq!(
        image_zoom_key(ImageZoom::scaled(3.0), &key("0"), ZOOM_BODY, image),
        Some(ImageZoom::FIT)
    );
    assert_eq!(
        image_zoom_key(ImageZoom::FIT, &key("1"), ZOOM_BODY, image),
        Some(ImageZoom::scaled(1.0))
    );
    assert_eq!(
        image_zoom_key(ImageZoom::FIT, &key("j"), ZOOM_BODY, image),
        None,
        "and every other key falls through to whatever the surface does with it"
    );
    assert_eq!(
        image_zoom_key(
            ImageZoom::FIT,
            &Key::Named(NamedKey::ArrowDown),
            ZOOM_BODY,
            image
        ),
        None
    );
}

#[test]
fn the_meta_line_says_how_the_picture_is_being_looked_at() {
    assert_eq!(
        image_meta_sentence(
            Some((1280, 800)),
            None,
            Some("PNG"),
            Some(219_136),
            Some("Fit")
        ),
        Some("1280 × 800 · PNG · 214 KB · Fit".to_owned())
    );
    assert_eq!(
        image_zoom_caption(ZOOM_BODY, [400, 300], ImageZoom::FIT),
        "Fit"
    );
    assert_eq!(
        image_zoom_caption(ZOOM_BODY, [400, 300], ImageZoom::scaled(1.5)),
        "150%"
    );
    assert_eq!(
        image_zoom_caption(ZOOM_BODY, [400, 300], ImageZoom::scaled(1.0 / 3.0)),
        "33%",
        "rounded to whole percent; the wheel's own steps land nowhere round"
    );
    assert_eq!(
        image_meta_sentence(None, None, None, None, Some("Fit")),
        None,
        "and the word is never said alone — there would be no sentence around it"
    );
}

/// RED — **the foot says both sizes when the picture on the glass is a
/// reduction of the file** (owner's ruling 2026-09-12; `bt_term`'s test of
/// the same name is the decode half).
///
/// A picture over the pixel budget is reduced rather than refused, so the
/// pane holds fewer pixels than the file has — and the number a reader wants
/// under a 24 megapixel photograph is the photograph's. Both are true, they
/// answer different questions, and the second rides on the sentence that was
/// already there rather than in a corner of its own.
///
/// RED GATE: drop the `shown` field and the first assertion reads
/// `6000 × 4000 · PNG · 92 MB · Fit` — a pane claiming to be drawing
/// 24 million pixels it does not have.
#[test]
fn a_picture_over_the_pixel_budget_is_reduced_to_fit_it_and_says_so() {
    assert_eq!(
        image_meta_sentence(
            Some((6000, 4000)),
            Some((5016, 3344)),
            Some("PNG"),
            Some(96_468_992),
            Some("Fit")
        ),
        Some("6000 × 4000 · shown at 5016 × 3344 · PNG · 92.0 MB · Fit".to_owned())
    );
    assert_eq!(
        image_meta_sentence(Some((1280, 800)), None, Some("PNG"), Some(219_136), None),
        Some("1280 × 800 · PNG · 214 KB".to_owned()),
        "and an ordinary picture says its one size, as it always has"
    );
    assert_eq!(
        image_meta_sentence(None, Some((5016, 3344)), None, None, Some("Fit")),
        None,
        "a size the window has not been told cannot be qualified by a second one"
    );
}

/// RED — **a picture in a Markdown page and a picture on the glance card are
/// read by the one decoder, under the one pair of caps** (owner's ruling
/// 2026-09-12).
///
/// The ruling set two numbers for a picture file and said the Markdown path
/// and the glance card get them "because they read the same decoder". That
/// is a claim about this file's shape, and it is asserted as one: the three
/// surfaces that show a picture — a page's block, the card, and a picture
/// pane — read `peek_cache` under the decoder's own key for a file, and none
/// of them names a byte cap of its own.
///
/// MUTATION: give any of the three its own cap — a length test before it
/// asks, a pixel test after it is answered — and the inner loop goes red.
#[test]
fn the_markdown_page_and_the_glance_card_read_the_same_caps() {
    let page = free_fn_body("answer_one_picture");
    let card = method_body("Runtime", "file_peek_fitted_pixels");
    let pane = method_body("Runtime", "refit_preview_picture");
    for (surface, text) in [("the page", page), ("the card", card), ("the pane", pane)] {
        assert!(
            text.contains("peek_cache"),
            "{surface} does not read the one decode store"
        );
        assert!(
            text.contains("local_image_path_key("),
            "{surface} does not key by the decoder's own identity for a file"
        );
        for cap in [
            concat!("MAX_LOCAL_IMAGE", "_FILE_BYTES"),
            concat!("MAX_INLINE_IMAGE", "_RGBA_BYTES"),
            concat!("MAX_BACKGROUND_IMAGE", "_RGBA_BYTES"),
            concat!("MAX_INLINE_IMAGE", "_BYTES"),
        ] {
            assert!(
                !text.contains(cap),
                "{surface} applies {cap} for itself; the caps belong to the decoder"
            );
        }
    }
}

#[test]
fn fit_follows_a_resize_and_a_chosen_scale_does_not() {
    let image = [2000_u32, 1000];
    let narrow = [0.0, 0.0, 500.0, 250.0];
    assert_ne!(
        rect_size(image_destination(ZOOM_BODY, image, ImageZoom::FIT)),
        rect_size(image_destination(narrow, image, ImageZoom::FIT)),
        "Fit is a mode: it is re-solved against whatever body it is asked about"
    );
    let chosen = ImageZoom {
        mode: ImageZoomMode::Scale(0.6),
        pan: [400.0, 0.0],
    };
    assert_eq!(
        rect_size(image_destination(ZOOM_BODY, image, chosen)),
        rect_size(image_destination(narrow, image, chosen)),
        "a chosen scale is a number the user asked for and survives the resize"
    );
    assert_close(
        image_clamped_pan(narrow, image, chosen)[0],
        (1200.0 - 500.0) / 2.0,
        "only the pan is re-clamped, to the road the new body leaves it",
    );
}

#[test]
fn a_picture_too_large_for_the_shared_budget_goes_soft_instead_of_vanishing() {
    assert_eq!(
        image_raster_cap([1920, 1200]),
        (1920, 1200),
        "an ordinary decode is 9 MiB and keeps every one of its pixels"
    );
    let (width, height) = image_raster_cap([6000, 4000]);
    assert!(
        u64::from(width) * u64::from(height) * 4
            <= (bt_viewport::MATH_TEXTURE_CACHE_BUDGET_BYTES as f64 * PREVIEW_IMAGE_TEXTURE_SHARE)
                as u64,
        "91 MiB of RGBA is capped to this picture's share of the shared budget"
    );
    assert!(
        ((width as f32 / height as f32) - 1.5).abs() < 0.01,
        "and the cap is a box the picture still fits exactly"
    );
    assert_eq!(image_raster_cap([0, 0]), (0, 0), "and nothing is nothing");
}

#[test]
fn the_hand_is_offered_exactly_when_the_picture_has_somewhere_to_go() {
    let image = [400_u32, 300];
    assert!(!image_is_pannable(ZOOM_BODY, image, ImageZoom::FIT));
    assert!(!image_is_pannable(ZOOM_BODY, image, ImageZoom::scaled(1.0)));
    assert!(
        image_is_pannable(ZOOM_BODY, image, ImageZoom::scaled(2.5)),
        "250% of 400×300 is 750 tall inside a 500 tall body"
    );
}

/// RED — **every zoom gesture asks the door that knows a video from a
/// picture** (user ruling 2026-08-27; §7.23).
///
/// `surface_takes_image_zoom` answers about the *surface* and is now half of
/// the rule: the other half is the content, because a frame decoded into
/// `VIDEO_FRAME_FIT_PX` has no "the file's own pixels" for a percentage to be
/// a multiple of. `Runtime::picture_takes_zoom` is both halves, and what has
/// to be true is that no gesture reaches past it to the surface half alone —
/// which is a claim about *which function a call site names*, so it is
/// asserted as text for `files_locate_door_tests`' reason.
///
/// RED GATE: put `surface_takes_image_zoom` back at the wheel's gate and this
/// names the file and the count — on the machine, a bare wheel over a video
/// magnifies it and the fact line underneath never mentions that it did.
#[test]
fn no_zoom_gesture_reaches_past_the_door_that_knows_a_video() {
    // One: inside `picture_takes_zoom`, which is the one place the surface
    // half is asked and then AND-ed with the content's. The count is of
    // *calls*, with the declaration left out, so this test's own two calls of
    // it are outside the answer for the reason that matters — they are not in
    // the product.
    assert_eq!(
        in_product(&free_calls_of("surface_takes_image_zoom")),
        1,
        "a zoom gesture is asking the surface half alone — it must ask \
             `picture_takes_zoom`, which is the surface half AND the content's"
    );
    for gate in [
        "press_preview_image",
        "preview_image_zoom",
        "set_preview_image_zoom",
    ] {
        assert!(
            method_body("Runtime", gate).contains("self.picture_takes_zoom(surface)"),
            "`{gate}` must ask the content-aware door"
        );
    }
}

#[test]
fn the_glance_card_mirrors_at_fit_and_cannot_be_zoomed() {
    assert!(!surface_takes_image_zoom(PreviewSurface::Peek));
    assert!(surface_takes_image_zoom(seat_of(TAB_ONE, SeatId(1))));
    assert!(surface_takes_image_zoom(PreviewSurface::Float(7)));
}

#[test]
fn two_surfaces_of_one_buffer_hold_their_own_zoom() {
    let mut panes = PreviewPanes::default();
    let (left, right) = (seat_of(TAB_ONE, SeatId(4)), seat_of(TAB_ONE, SeatId(5)));
    panes.entry(left).zoom = ImageZoom {
        mode: ImageZoomMode::Scale(2.5),
        pan: [30.0, -12.0],
    };
    assert_eq!(
        panes.entry(right).zoom,
        ImageZoom::FIT,
        "the zoom is the surface's, exactly as the scroll is (ruling 8⑧)"
    );
    assert_eq!(
        panes.get(left).map(|pane| pane.zoom.mode),
        Some(ImageZoomMode::Scale(2.5)),
        "and the one that was set kept it"
    );
    assert_eq!(
        PreviewPane::default().zoom,
        ImageZoom::FIT,
        "a surface that has never been looked at is looking at Fit"
    );
}

/// PIN — a `cd` relabels the tab, and does not wait for something unrelated
/// to repaint the strip.
///
/// Red gate: watching `window_title` alone — which is what
/// [`drain_leaf_pty`] did — passes every other test in this file and leaves
/// the tab strip showing the folder a shell left. Measured on a real
/// Command Prompt pane before the fix: `cd` into `C:\Program Files` left the
/// tab reading `BetterTerminal` while the prompt one line below it read
/// `C:\Program Files>`, and it stayed that way until a click happened to
/// repaint the chrome.
///
/// The pair is what the name stack reads, so the pair is what has to be
/// watched; either half moving is a tab that has to be relabelled.
#[test]
fn everything_a_tab_is_named_from_is_watched_for_change() {
    let mut leaf = leaf_saying("x");
    let start = leaf.name_evidence();

    // The folder layer — the one that names most tabs, and the one that was
    // not being watched.
    leaf.session.feed(b"\x1b]7;file:///D:\\src\x1b\\").unwrap();
    let moved = leaf.name_evidence();
    assert_ne!(moved, start, "an OSC 7 report moves the name");
    leaf.session
        .feed(b"\x1b]7;file:///D:\\other\x1b\\")
        .unwrap();
    assert_ne!(leaf.name_evidence(), moved, "and so does the next one");

    // The title layer, still watched.
    let before_title = leaf.name_evidence();
    leaf.session.feed(b"\x1b]0;vim main.rs\x07").unwrap();
    assert_ne!(leaf.name_evidence(), before_title);

    // And ordinary output moves neither, so this does not relabel the strip
    // on every byte a shell prints.
    let quiet = leaf.name_evidence();
    leaf.session.feed(b"hello\r\nworld\r\n").unwrap();
    assert_eq!(leaf.name_evidence(), quiet);

    // The two halves are told apart, because they drive different machines:
    // a title relabels chrome that is not written down, a folder is a field
    // of this leaf in `session.json` and has to reach the file.
    let (title_before, place_before) = leaf.name_evidence();
    leaf.session.feed(b"\x1b]0;vim other.rs\x07").unwrap();
    let (title_after, place_after) = leaf.name_evidence();
    assert_ne!(title_after, title_before, "the title moved");
    assert_eq!(place_after, place_before, "and the folder did not");
}

#[test]
fn reduced_motion_skips_the_landing_animation_outright() {
    // Mock-up 968 says so in as many words, and an animation with only a
    // `from` has nothing to hold: off means the tab, unwashed.
    let now = Instant::now();
    let mut landing = LandTween::default();
    landing.start(now, Motion::Reduced);
    assert_eq!(landing.sample(now, Motion::Reduced), (0.0, false));
}

/// PIN (user report, 2026-08-12) — **what looks like a scrollbar is one**.
///
/// The report is that the bar under a wide table could not be dragged. It
/// was an *indicator*: a proportional thumb on a track, drawn in exactly the
/// shape every application on the desk uses for a control, and wired to
/// nothing. This asks the three questions a scrollbar has to answer — can a
/// hand find it, does dragging it move the content in proportion, and does
/// it stop where the wheel stops — and it asks the first of them against the
/// rectangle the *painter* produced, because a thumb hit-tested anywhere
/// other than where it was drawn is the same defect wearing a fix.
///
/// MUTATIONS that must turn it red:
/// ① have `scroll_dragged_to` return the offset it was given instead
///    of reading the pointer — the drag writes nothing, which is the bug;
/// ② drop the `grab` widening in `scroll_bar` — the second question
///    goes red and two drawn pixels are the whole target again;
/// ③ paint the thumb from any arithmetic other than `scroll_bar` —
///    the first assertion goes red.
#[test]
fn the_bar_under_a_wide_block_is_a_thumb_a_hand_can_take_and_drag() {
    const SCALE: f32 = 1.0;
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(SCALE);
    let body = [0.0, 0.0, 300.0, 400.0];
    let page = body[2] - body[0] - metrics.padding_x * 2.0;
    let wide = page + 400.0;
    let blocks = [
        preview::MarkdownBlock::Paragraph(vec![preview::Span::plain("prose")]),
        preview::MarkdownBlock::Code {
            lang: None,
            text: "a very long line".to_owned(),
        },
    ];
    let layout: preview_viewport::Layout = [
        MarkdownBlockLayout::solid(metrics.line_height),
        MarkdownBlockLayout {
            width: wide,
            top: metrics.line_height + metrics.paragraph_gap,
            ..MarkdownBlockLayout::solid(metrics.line_height * 3.0)
        },
    ]
    .into();
    let overflow = wide - page;
    let offsets = [0.0, 0.0];
    let document = (&blocks[..], &layout);
    let bar_at = |at: [f32; 2]| {
        preview_block_bar_at(body, metrics, [0.0, 0.0], &offsets, document, SCALE, at)
    };

    // ① The thumb a hand finds is the thumb the painter drew.
    let built = markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&offsets),
        document,
        &palette,
    );
    let drawn = built
        .blocks
        .first()
        .expect("a block wider than its page scrolls inside itself")
        .quads
        .last()
        .expect("and wears a thumb")
        .rect;
    let on_thumb = [drawn[0] + 2.0, (drawn[1] + drawn[3]) / 2.0];
    let (index, bar) = bar_at(on_thumb).expect("the pointer on the drawn thumb finds one");
    assert_eq!(index, 1, "and it belongs to the wide block");
    assert_eq!(bar.thumb, drawn, "hit and paint are one geometry");

    // ② The band is wider than the rule — two drawn pixels are not a target.
    let above = [on_thumb[0], bar.thumb[1] - 2.0];
    assert!(
        bar.thumb[1] - 2.0 < bar.thumb[1] && bar_at(above).is_some(),
        "a hand landing just above the rule still has it"
    );
    assert!(
        bar_at([on_thumb[0], bar.grab[1] - 4.0]).is_none(),
        "and the tolerance is a tolerance, not the whole block"
    );

    // ③ Dragging maps the track onto the overflow, linearly, and stops
    //    where the wheel stops.
    let grab = on_thumb[0] - bar.thumb[0];
    let dragged_to = |x: f32| preview::scroll_dragged_to(&bar, x, grab);
    assert_eq!(dragged_to(on_thumb[0]), 0.0, "at rest it has not moved");
    assert!(
        (dragged_to(on_thumb[0] + bar.travel / 2.0) - overflow / 2.0).abs() < 0.5,
        "half the track is half the overflow"
    );
    assert!(
        (dragged_to(on_thumb[0] + bar.travel) - overflow).abs() < 0.5,
        "the whole track is the whole overflow"
    );
    assert_eq!(
        dragged_to(on_thumb[0] + bar.travel * 4.0),
        overflow,
        "and it stops at the block's own end"
    );
    assert_eq!(
        dragged_to(on_thumb[0] - 400.0),
        0.0,
        "and at its own start — the wheel's two clamps, exactly"
    );
}

/// PIN (user report, 2026-08-25) — **a display formula is as tall as its
/// picture, and stands on its source until that picture arrives.**
///
/// The two halves are one rule seen at two moments, and getting either wrong
/// is the hole a list once left in the middle of `DESIGN.md`: the pass that
/// reserves the room and the pass that fills it must agree. Before the
/// engine answers there is nothing to draw but the LaTeX the author wrote,
/// and it takes as many lines as it has.
///
/// The width matters as much as the height: it is what makes a formula wider
/// than the measure scroll inside its own block instead of being cropped, on
/// the terms a table and a fence already have.
///
/// MUTATIONS: return `solid(picture.height_px)` without the width and the
/// third assertion goes red; keep the source-lines height when a picture is
/// in hand and the first goes red.
#[test]
fn a_display_formula_is_as_tall_as_its_picture_and_stands_on_its_source_until_it_comes() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let block = preview::MarkdownBlock::Math {
        source: "\\begin{aligned}\na &= b\n\\end{aligned}".to_owned(),
    };
    let mut never_measured = |_: &[bt_render::PreviewRun], _: f32, _: f32, _: f32| {
        unreachable!("a formula's block never asks the shaper anything")
    };
    let waiting = measure_markdown_block(
        &block,
        &MarkdownBlockIntrinsic::default(),
        400.0,
        metrics,
        PageArt {
            math: &DocumentMath::default(),
            pictures: &DocumentPictures::default(),
            theme: bt_render::Theme::Dark,
        },
        &mut never_measured,
    );
    assert_eq!(
        waiting.height,
        metrics.line_height * 3.0,
        "three lines of source, three lines of room",
    );
    assert_eq!(
        waiting.width, 0.0,
        "and nothing to scroll: the source folds to the page like any prose",
    );
    let math = one_picture(
        "\\begin{aligned}\na &= b\n\\end{aligned}",
        MathMode::Display,
        metrics.font_size,
        (900, 64, 40.0),
    );
    let set = measure_markdown_block(
        &block,
        &MarkdownBlockIntrinsic::default(),
        400.0,
        metrics,
        PageArt {
            math: &math,
            pictures: &DocumentPictures::default(),
            theme: bt_render::Theme::Dark,
        },
        &mut never_measured,
    );
    assert_eq!(
        (set.height, set.width),
        (64.0, 900.0),
        "once the picture is in hand the block is exactly the picture",
    );
}

/// PIN (same report) — **the page draws the picture where it reserved the
/// room, centred on the measure, and nothing else.**
///
/// Centring is what display mathematics is in every dialect that has it. The
/// second half is the one a reader would notice first if it were wrong: the
/// source text must be *gone* once the picture is there, or the LaTeX prints
/// underneath its own rendering.
///
/// MUTATIONS: push the source paragraphs on both arms and the last assertion
/// goes red; left-align the picture and the first does.
#[test]
fn a_display_formula_draws_centred_and_stops_printing_its_own_source() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 400.0, 400.0];
    let blocks = [preview::MarkdownBlock::Math {
        source: "E = mc^2".to_owned(),
    }];
    let math = one_picture(
        "E = mc^2",
        MathMode::Display,
        metrics.font_size,
        (120, 30, 20.0),
    );
    let layout: preview_viewport::Layout = [MarkdownBlockLayout {
        width: 120.0,
        ..MarkdownBlockLayout::solid(30.0)
    }]
    .into();
    let rendered = build_preview_markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &[],
            layout: &layout,
            live: MarkdownLive::default(),
        },
        &palette,
        PageArt {
            math: &math,
            pictures: &DocumentPictures::default(),
            theme: bt_render::Theme::Dark,
        },
    );
    let (left, right) = preview::markdown_measure_box(body, metrics);
    assert_eq!(rendered.body.rasters.len(), 1, "one formula, one picture");
    let drawn = &rendered.body.rasters[0];
    assert_eq!(
        drawn.rect[0] - left,
        right - drawn.rect[2],
        "the air either side of it is equal, which is what centred means",
    );
    assert_eq!(
        (drawn.rect[2] - drawn.rect[0], drawn.rect[3] - drawn.rect[1]),
        (120.0, 30.0),
        "drawn at its own size, never stretched to the column",
    );
    assert!(
        rendered.body.paragraphs.is_empty(),
        "and the LaTeX is not printed under its own picture: {:?}",
        rendered.body.paragraphs,
    );
    // The other moment: no picture, and the source is all there is.
    let waiting = build_preview_markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &[],
            layout: &layout,
            live: MarkdownLive::default(),
        },
        &palette,
        PageArt {
            math: &DocumentMath::default(),
            pictures: &DocumentPictures::default(),
            theme: bt_render::Theme::Dark,
        },
    );
    assert!(waiting.body.rasters.is_empty());
    assert_eq!(
        waiting
            .body
            .paragraphs
            .iter()
            .flat_map(|paragraph| paragraph.runs.iter().map(|run| run.text.as_str()))
            .collect::<Vec<_>>(),
        vec!["E = mc^2"],
        "what the author wrote, until there is something better to show",
    );
}

/// One picture already decoded, in the shape a page draws it from.
///
/// The pixels are never looked at: every assertion about a picture here is
/// about the *box* — how much room the page gave it and where it put it.
fn one_image(source: &str, native: [u32; 2]) -> DocumentPictures {
    let mut pictures = DocumentPictures::default();
    pictures.by_source.insert(
        source.to_owned(),
        MarkdownPicture::Ready {
            key: format!("test:{source}"),
            content: format!("test:{source}"),
            rgba: Arc::from(vec![0_u8; (native[0] * native[1] * 4) as usize].into_boxed_slice()),
            raster: native,
            native,
        },
    );
    pictures
}

/// RED — **the reported page's own screenshots fit the window's budget, and
/// a frame that changed nothing asks for nothing** (user report 2026-08-29,
/// defect #186: 「预览 pane 独占一个 tab 后很卡」).
///
/// The document is `README.zh-CN.md` and the pictures are its own: five
/// 3200×2000 screenshots and one 3200×3240 plate, drawn into the 77em
/// measure a 4K screen at 200% gives a full-window pane — 1404 physical
/// pixels, which is what the report's geometry works out to and the number
/// the sizes below are derived from rather than typed.
///
/// The claim is the one the report needs: **once the exact-size rasters have
/// landed, a page standing still or scrolling costs zero resamples, zero
/// uploads and zero re-flows.** Each of the three is a counter here rather
/// than a stopwatch:
///
/// * a **re-flow** is [`PageArtKey::picture_generation`] moving, because that
///   is the only thing about a picture a document key can notice;
/// * a **resample** is an entry in [`MarkdownPictures::owed`];
/// * an **upload** is an eviction — a raster that leaves this cache is one
///   the next frame asks the GPU for again.
///
/// The measurement behind it, taken on the machine the same day with
/// `BT_PERF_TRACE`: peak 27.3MB of the 64MB texture budget,
/// `math_texture_evictions=0`, `math_texture_refusals=0`, no atlas repack,
/// and zero CPU over six seconds of a page standing still.
///
/// MUTATIONS: cut [`MARKDOWN_PICTURE_BUDGET_BYTES`] below this set and the
/// budget assertion goes red — and the eviction it lets in ticks the
/// generation, which re-flows the page, which asks for the raster again,
/// which evicts, for as long as the pane is open; drop the `.min(1.0)` out
/// of [`markdown_image_extent`] and every raster is the decode's own 25.6MB
/// and three of them do not fit anywhere.
#[test]
fn a_page_of_screenshots_costs_no_resample_and_no_reflow_the_second_time() {
    // 77em of a 13px body at 200% — `preview::PREVIEW_PROSE_MEASURE_EM`
    // through `preview::markdown_metrics`, which is where the number is
    // decided.
    let measure = seats::preview_markdown_metrics(2.0).measure;
    let mut pictures = MarkdownPictures::default();
    let mut keys = Vec::new();
    for (index, native) in [
        [3200_u32, 2000],
        [3200, 2000],
        [3200, 2000],
        [3200, 2000],
        [3200, 2000],
        [3200, 3240],
    ]
    .into_iter()
    .enumerate()
    {
        // The very arithmetic `Runtime::resolve_document_pictures` does:
        // the drawn box, capped at the decode's own pixels and at the share
        // of the texture budget one picture may take.
        let [drawn_width, drawn_height] = markdown_image_extent(native, measure, true);
        let (cap_width, cap_height) = image_raster_cap(native);
        let (width_px, height_px) = bt_render::preview_image_extent(
            (drawn_width.round().max(1.0) as u32).min(cap_width),
            (drawn_height.round().max(1.0) as u32).min(cap_height),
            native[0],
            native[1],
        )
        .expect("a picture the page can draw");
        let key = MarkdownRasterKey {
            content: format!("shot-{index}"),
            width_px,
            height_px,
        };
        pictures.land(
            key.clone(),
            MarkdownRaster::Ready {
                key: format!("texture-{index}"),
                rgba: Arc::from(
                    vec![0_u8; (width_px as usize) * (height_px as usize) * 4].into_boxed_slice(),
                ),
                width_px,
                height_px,
            },
        );
        keys.push(key);
    }

    assert_eq!(
        pictures.rasters.len(),
        keys.len(),
        "the whole page fits: nothing was evicted to make room for the last \
             picture, at {} bytes resident",
        pictures.resident_bytes
    );
    assert!(
        pictures.resident_bytes <= MARKDOWN_PICTURE_BUDGET_BYTES,
        "{} bytes resident against a {MARKDOWN_PICTURE_BUDGET_BYTES}-byte budget",
        pictures.resident_bytes
    );

    // And now the frame that changed nothing.
    let settled = pictures.generation;
    for key in &keys {
        assert!(
            matches!(pictures.raster(key), Some(MarkdownRaster::Ready { .. })),
            "every picture is in hand at the size the page draws it"
        );
    }
    assert_eq!(
        pictures.generation, settled,
        "so nothing about the page came out differently, and no document key \
             moved: zero re-flows"
    );
    assert!(pictures.owed.is_empty(), "zero resamples owed the worker");
    assert!(
        pictures.settle_deadline.is_none(),
        "and therefore no wake-up asked for either"
    );
}

/// RED — **a page asks for each of its pictures once, however small the
/// decode cache is** (user report 2026-09-10, `docs/DESIGN.md` §7.1.3u).
///
/// RED EVIDENCE. The reach a page asks for is counted in *pictures*
/// ([`MARKDOWN_PICTURE_MARGIN`]); the store the decodes land in is bounded
/// in *bytes* ([`MAX_PEEK_CACHE_BYTES`]). Nothing reconciled the two, so a
/// README whose screenshots are 3200×2000 — nine of them, 24 MiB each,
/// against a 192 MiB ceiling — put this window in a livelock: every decode
/// that landed evicted one the page was still drawing, the miss read as
/// *never asked*, and the page asked again. Measured on this machine, with
/// that very page in a 1920×1200 window at scale 2: **1922 decodes in 45
/// seconds**, one core pinned, 460 document re-flows a second, and not one
/// redraw — a window frozen on its last picture while the process went on
/// answering messages.
///
/// The fixture is that story at the size a test can hold: two pictures, a
/// cache that can carry one of them, and twelve rounds of the loop the
/// runtime runs — a decode lands, every picture on the page is resolved
/// again, the requests that came out of it go to the worker. What is
/// asserted is the count: **two pictures, two reads, for ever**.
///
/// MUTATION: drop the `standing` arm from [`answer_one_picture`]'s `None`
/// branch — read a miss as "never asked" again — and the count climbs by one
/// per round, which is the report.
#[test]
fn a_page_asks_for_each_picture_once_however_small_the_decode_cache_is() {
    /// Each decode, in bytes. Two of them do not fit under the budget
    /// below, which is the whole fixture.
    const PIXELS: usize = 3 * 1024 * 1024;
    const BUDGET: u64 = 4 * 1024 * 1024;
    const ROUNDS: usize = 12;
    let paths = [
        PathBuf::from(r"D:\proj\shots\a.png"),
        PathBuf::from(r"D:\proj\shots\b.png"),
    ];
    let mut peek = PeekCache::with_budget(BUDGET);
    let mut rasters = MarkdownPictures::default();
    let mut standing = [MarkdownPicture::Loading, MarkdownPicture::Loading];
    let mut asked = 0usize;
    // What the worker owes this window: one decode lands per round, which is
    // what the runtime does — `complete_peek_image` files one answer and
    // rebuilds the page before the next lands.
    let mut inbox: Vec<usize> = Vec::new();
    for _ in 0..ROUNDS {
        if !inbox.is_empty() {
            let index = inbox.remove(0);
            peek.insert(
                bt_term::normalized_local_image_path_key(&paths[index]),
                PeekCacheEntry::Ready {
                    key: format!("content-{index}"),
                    rgba: Arc::from(vec![0u8; PIXELS].into_boxed_slice()),
                    width_px: 1024,
                    height_px: 768,
                    native_size: None,
                },
            );
        }
        for index in 0..paths.len() {
            let mut needs_pixels = false;
            standing[index] = answer_one_picture(
                &mut peek,
                &mut rasters,
                Some(&standing[index]),
                &paths[index],
                false,
                800.0,
                Instant::now(),
                &mut needs_pixels,
            )
            .picture;
            if needs_pixels {
                asked += 1;
                inbox.push(index);
                // The `Pending` the runtime files behind a posted request.
                peek.insert(
                    bt_term::normalized_local_image_path_key(&paths[index]),
                    PeekCacheEntry::Pending,
                );
            }
        }
    }
    assert_eq!(
        asked,
        paths.len(),
        "a page that has been answered asked again: {asked} reads for \
             {} pictures over {ROUNDS} rounds — the decode cache let one go and \
             the page took that for never having asked",
        paths.len(),
    );
    // And it is still drawing both: the answer outlives the pixels.
    for (index, picture) in standing.iter().enumerate() {
        assert!(
            matches!(picture, MarkdownPicture::Ready { .. }),
            "picture {index} went blank when the cache let its pixels go: {picture:?}"
        );
    }
    assert!(
        peek.bytes_held() <= BUDGET,
        "and the cache is still inside its budget: {} bytes",
        peek.bytes_held()
    );
}

/// RED — **typing into a page asks for none of its pictures again** (user
/// report 2026-09-11, `docs/DESIGN.md` §7.1.3u).
///
/// RED EVIDENCE. §7.1.3u stopped a page re-asking for a decode the byte-
/// bounded cache had let go of, and *opening* the reported README stopped
/// freezing. Clicking into one of its paragraphs still did: 82 seconds of
/// processor in about 90, the window thread walking
/// `complete_peek_image` → a full re-flow → the shaper, for ever.
///
/// The reason is one string. A picture's answer carries the identity the
/// **GPU** knows its pixels by, and the moment the exact-size resample lands
/// that identity stops being the file's content key and becomes
/// `<content>@<width>x<height>` ([`bt_term::display_texture_key`]). The page
/// then held an answer that could no longer name its own file: with the
/// decode evicted, [`answer_one_picture`] read that texture key *as* the
/// content key, asked [`MarkdownPictures`] a question about a file that does
/// not exist, missed, and read the miss as **never asked**. So the page was
/// answered, and settled, and the first rebuild after it settled — which is
/// exactly what a press seating a caret is (§7.1.3q keys the caret's block
/// beside the width, so entering an edit is a re-flow) — sent every one of
/// those reads out again, and each decode landing evicted the next page's
/// and rebuilt the document.
///
/// The fixture is that arc: two pictures, a decode cache that can hold one
/// of them, the resamples landing the way the worker lands them, and then
/// ten keystrokes' worth of rebuilds. The claim is the count — **two
/// pictures, two reads, and typing adds none**.
///
/// MUTATIONS, both measured 2026-09-11. Read [`MarkdownPicture::key`]
/// instead of [`MarkdownPicture::content`] in `answer_one_picture`'s
/// standing arm and the count goes to three: `editing sent 1 reads out again
/// for pictures the page was already drawing — 2 before the first keystroke,
/// 3 after ten`. **One and not ten is the fixture and not the defect** —
/// there are two pictures here and the `Pending` the extra read files stays
/// in a cache nothing else is inserting into, so the ask cannot come round
/// again; on the real page, where a landing decode evicts the next picture's,
/// that one read is the first turn of a rotation that never finishes. Drop
/// the `held_exactly` arm instead and it is the picture whose decode the
/// cache let go of that asks, on the same keystroke, for the same reason one
/// cache down.
#[test]
fn ten_keystrokes_ask_for_no_picture_the_page_is_already_drawing() {
    /// Each decode. Two do not fit under the budget below, which is what
    /// puts the page in the state the report is about.
    const PIXELS: usize = 3 * 1024 * 1024;
    const BUDGET: u64 = 4 * 1024 * 1024;
    const NATIVE: [u32; 2] = [1024, 768];
    const MEASURE: f32 = 800.0;
    const KEYSTROKES: usize = 10;
    let paths = [
        PathBuf::from(r"D:\proj\shots\a.png"),
        PathBuf::from(r"D:\proj\shots\b.png"),
    ];
    let mut peek = PeekCache::with_budget(BUDGET);
    let mut rasters = MarkdownPictures::default();
    let mut standing = [MarkdownPicture::Loading, MarkdownPicture::Loading];
    let mut asked = 0usize;
    let mut inbox: Vec<usize> = Vec::new();

    // One rebuild of the page: every picture resolved, every read that came
    // out of it posted, the way `rebuild_preview_document` does it.
    let rebuild = |peek: &mut PeekCache,
                   rasters: &mut MarkdownPictures,
                   standing: &mut [MarkdownPicture; 2],
                   asked: &mut usize,
                   inbox: &mut Vec<usize>| {
        for index in 0..paths.len() {
            let mut needs_pixels = false;
            standing[index] = answer_one_picture(
                peek,
                rasters,
                Some(&standing[index]),
                &paths[index],
                true,
                MEASURE,
                Instant::now(),
                &mut needs_pixels,
            )
            .picture;
            if needs_pixels {
                *asked += 1;
                inbox.push(index);
                peek.insert(
                    bt_term::normalized_local_image_path_key(&paths[index]),
                    PeekCacheEntry::Pending,
                );
            }
        }
    };

    // ① The page opens: the reads go out, the decodes land one at a time,
    // and the second one evicts the first.
    for _ in 0..6 {
        if !inbox.is_empty() {
            let index = inbox.remove(0);
            peek.insert(
                bt_term::normalized_local_image_path_key(&paths[index]),
                PeekCacheEntry::Ready {
                    key: format!("content-{index}"),
                    rgba: Arc::from(vec![0u8; PIXELS].into_boxed_slice()),
                    width_px: NATIVE[0],
                    height_px: NATIVE[1],
                    native_size: None,
                },
            );
        }
        rebuild(
            &mut peek,
            &mut rasters,
            &mut standing,
            &mut asked,
            &mut inbox,
        );
    }
    assert_eq!(asked, paths.len(), "§7.1.3u's own count, before the edit");

    // ② The exact-size resamples land, the way the scale worker lands them
    // — under the content key, carrying the *texture* key. This is the step
    // that used to poison the answer.
    let owed: Vec<MarkdownRasterRequest> =
        rasters.owed.drain().map(|(_, request)| request).collect();
    assert!(
        !owed.is_empty(),
        "the page owed the lane its exact-size passes"
    );
    for request in owed {
        let key = request.key.clone();
        rasters.land(
            key.clone(),
            MarkdownRaster::Ready {
                key: bt_term::display_texture_key(&key.content, key.width_px, key.height_px),
                rgba: Arc::from(
                    vec![0u8; (key.width_px as usize) * (key.height_px as usize) * 4]
                        .into_boxed_slice(),
                ),
                width_px: key.width_px,
                height_px: key.height_px,
            },
        );
    }
    rebuild(
        &mut peek,
        &mut rasters,
        &mut standing,
        &mut asked,
        &mut inbox,
    );
    let settled = asked;

    // ③ And the raster store lets go of everything, which is the other
    // bounded cache in this story saying the same word the decode store
    // said. The page is holding those pixels itself.
    rasters = MarkdownPictures::default();

    // ④ And now the reader clicks into a paragraph and types. Every
    // keystroke is a rebuild; the page has been answered; nothing is owed
    // anybody.
    for _ in 0..KEYSTROKES {
        rebuild(
            &mut peek,
            &mut rasters,
            &mut standing,
            &mut asked,
            &mut inbox,
        );
    }
    assert_eq!(
        asked,
        settled,
        "editing sent {} reads out again for pictures the page was already \
             drawing — {settled} before the first keystroke, {asked} after ten",
        asked - settled,
    );
    assert_eq!(
        asked,
        paths.len(),
        "and the whole life of the page is one read per picture"
    );
    for (index, picture) in standing.iter().enumerate() {
        assert!(
            matches!(picture, MarkdownPicture::Ready { .. }),
            "picture {index} went blank while the reader typed: {picture:?}"
        );
    }
}

/// RED — **a decode landing for a picture the page already draws re-flows
/// nothing** (user report 2026-09-11, `docs/DESIGN.md` §7.1.3u).
///
/// RED EVIDENCE. The other half of the same freeze. `complete_peek_image`
/// ticked [`MarkdownPictures::generation`] for any file **a page's pictures
/// came from** — which since §7.1.3u is a much larger set than the files a
/// page is still *waiting* for, because a page now keeps the answer it was
/// given. So a decode arriving for a screenshot already on the glass re-laid
/// the whole document out, and laying a page of Chinese prose out is a
/// re-shape of every paragraph in it through the fallback stack — the
/// expensive half of each of the 82 seconds the report measured.
///
/// The seam is [`DocumentPictures::loading`], which is what
/// [`Runtime::markdown_pictures_awaited`] reads and what the generation is
/// ticked off. Here it is asked of the resolve itself: the first pass over
/// the page has nothing to draw and says so; the second, holding the answer,
/// asks for nothing and **waits for nothing**.
///
/// MUTATION: write `files` where `loading` is written and the second pass
/// names the file again — every completion re-flows the page, which is the
/// report.
#[test]
fn a_decode_that_lands_for_a_picture_the_page_holds_owes_it_no_reflow() {
    let blocks = preview::parse_markdown("![a shot](shots/one.png)\n");
    let document = host_path(r"D:\proj\README.md");
    let document = document.as_path();
    let waiting = resolve_document_pictures(
        &blocks,
        Some(document),
        bt_render::Theme::Dark,
        PictureReach::from_the_top(),
        &DocumentPictures::default(),
        &mut |_, _, _| PagePicture {
            picture: MarkdownPicture::Loading,
            waiting: Some(PictureWaitFor::Pixels),
        },
    );
    assert_eq!(
        waiting.awaited().count(),
        1,
        "a page with nothing to draw is waiting for the file it asked for"
    );
    assert_eq!(
        waiting.files,
        waiting.awaited().cloned().collect::<BTreeSet<_>>(),
        "and while it is waiting the two sets are the same one"
    );

    let holding = resolve_document_pictures(
        &blocks,
        Some(document),
        bt_render::Theme::Dark,
        PictureReach::from_the_top(),
        &one_image("shots/one.png", [1024, 768]),
        &mut |_, _, standing| {
            PagePicture::drawn(
                standing
                    .cloned()
                    .expect("the answer this page was already given"),
            )
        },
    );
    assert_eq!(
        holding.files.len(),
        1,
        "the page still stands on that file, so the watch still follows it"
    );
    assert_eq!(
        holding.awaited().count(),
        0,
        "but it is waiting for nothing, so a decode landing owes it no \
             re-flow: {:?}",
        holding.waiting,
    );
}

/// One surface holding a markdown page whose pictures are these.
fn a_page_holding(pictures: DocumentPictures) -> PreviewPane {
    PreviewPane {
        doc: PreviewDocument::Markdown {
            blocks: Vec::new(),
            ranges: Vec::new(),
            maps: Vec::new(),
            source: SourceBlocks::default(),
            intrinsic: Vec::new(),
            layout: preview_viewport::Layout::default(),
            math: DocumentMath::default(),
            pictures,
            wrap: Arc::default(),
        },
        ..PreviewPane::default()
    }
}

/// RED — **a decode that changes no intrinsic re-flows nothing** (adversarial
/// review 2026-09-11, row RB-6).
///
/// RED EVIDENCE. The other half of the ticket above, and the reason the
/// dependency is a set of its own rather than another entry in `loading`.
/// The generation every page is re-keyed by is ticked off
/// [`Runtime::markdown_pictures_awaited`], which reads `loading`; a page that
/// is only waiting for a picture to get *sharper* already knows how tall the
/// block is, so the decode's arrival moves nothing on the page and owes it no
/// re-flow. Laying a page of prose out again is a re-shape of every paragraph
/// in it through the fallback stack, which is the expensive half of the 82
/// seconds §7.1.3u ② measured.
///
/// MUTATION: write the sharpening file into `loading` as well and the page is
/// named to the completion as one that must be laid out again — a whole
/// document re-flow for a picture that is already on the glass at the size it
/// is drawn.
#[test]
fn a_completion_that_changes_no_intrinsic_does_not_reflow_the_document() {
    let fixture = a_page_that_wants_a_sharper_picture();
    let holder = a_page_holding(fixture.page.clone());
    assert!(
        fixture
            .page
            .sharpening()
            .any(|(file, _)| file == &fixture.file),
        "the page is waiting on that decode — it asked for it itself"
    );
    assert!(
        picture_files_of(std::iter::once(&holder)).contains(&fixture.file),
        "and it stands on the file, so the watch follows it"
    );
    assert!(
        !pictures_awaited_by(std::iter::once(&holder)).contains(&fixture.file),
        "but it is not waiting to *see* it, so the decode landing owes it no \
             re-flow: what it is waiting for is one exact-size pass",
    );
}

/// RED — **the glance card's document is in the awaited set** (adversarial
/// review 2026-09-11, row RB-5; §7.1.3u ③).
///
/// RED EVIDENCE. Three walks that decide what a page is owed —
/// [`Runtime::markdown_pictures_awaited`], [`Runtime::markdown_picture_files`]
/// and the standing-answer clearing in [`Runtime::forget_the_picture_in`] —
/// walked `tab.preview_panes`, and [`PreviewSurface::Peek`] is the one
/// surface not in any tab's map: the card is the *window's*, one pointer and
/// one card, so its view lives on [`WindowRuntime::peek_pane`]. The card
/// really does build a markdown document, so a hover over a `.md` file with
/// an uncached picture in it stayed on placeholders — nothing ticked the
/// generation for the decode it was waiting for — and its standing answers
/// survived the file moving under them.
///
/// Two claims, because the defect has two faces: that the walk finds the
/// card, and that the three places are built from that one walk rather than
/// each spelling it again. The second is asserted as text for
/// [`one_door_decides_which_decoder_a_hover_and_a_pane_ask`]'s reason: what
/// is being pinned is *which walk a set is built from*, and no value any
/// assertion can read says that.
///
/// MUTATION: leave `peek_pane` out of [`documents_held_in`] and the first
/// assertion goes red; spell a `tab.preview_panes` walk into any of the three
/// again and the second does; take the `documents_held` call out of one of them
/// and the third does. A `tab.preview_panes` written in a *comment* inside one
/// of the three is prose and leaves it green.
#[test]
fn the_glance_cards_document_is_in_the_awaited_set() {
    let file = PathBuf::from(r"D:\proj\shots/one.png");
    let mut pictures = DocumentPictures::default();
    pictures.files.insert(file.clone());
    pictures
        .waiting
        .insert(file.clone(), PictureWaitFor::Pixels);
    let card = a_page_holding(pictures);
    // No tabs at all: what is being asked is whether the card is a holder,
    // and a window with a card and nothing else is the plainest way to ask.
    assert!(
        pictures_awaited_by(documents_held_in(&[], &card)).contains(&file),
        "the card is waiting for that picture, and nothing said so"
    );
    assert!(
        picture_files_of(documents_held_in(&[], &card)).contains(&file),
        "and it stands on the file, so the watch follows it"
    );

    // **The prohibition, asked of each door's own bytes.**
    //
    // It used to be `format!("tab{}.preview_panes", ".")`, assembled at run
    // time so that this file's own text would not be one of the sites it
    // counted — and the separator went on the wrong side, so the needle was
    // `tab..preview_panes` and the half that was supposed to go red never
    // could. `needle!` is what that assembly was reaching for: it excludes the
    // one expression that built the needle (§2.6) and nothing else, so the
    // spelling can be written the way the product would write it.
    //
    // `View::CodeKeepingLiterals`, because a sentence in a comment about the
    // walk that used to be here is prose and not a walk.
    for door in [
        "markdown_pictures_awaited",
        "markdown_picture_files",
        "forget_the_picture_in",
    ] {
        let walk = found_in(
            needle!(Pattern::text("tab.preview_panes")),
            View::CodeKeepingLiterals,
            Scope::Item(ItemQuery::method("Runtime", door)),
        );
        assert!(
            walk.is_empty(),
            "`{door}` walks the tabs' panes for itself, so the card is not in \
             it:\n{}",
            walk.report(source())
        );
    }
    // **And each door is built from the one walk, by the call it makes.**
    // `contains("documents_held")` was the whole of this before, and it is a
    // prefix of `documents_held_in` and of `documents_held_mut` — so it could
    // not tell the walk that names the card from the two that do not.
    for (door, call) in [
        (
            "markdown_pictures_awaited",
            "pictures_awaited_by(self.documents_held())",
        ),
        (
            "markdown_picture_files",
            "picture_files_of(self.documents_held())",
        ),
        (
            "forget_the_picture_in",
            "forget_standing_answers(self.documents_held_mut(), path)",
        ),
    ] {
        let text = method_body("Runtime", door);
        assert!(
            text.contains(call),
            "`{door}` must be built from the one walk over every holder, as \
             `{call}`:\n{text}"
        );
    }
    assert!(
        method_body("Runtime", "documents_held").contains("peek_pane"),
        "and the one walk is the one that names the card"
    );
}

/// RED — **a watched file moving clears the glance card's standing answers**
/// (adversarial review 2026-09-11, row RB-5; §7.1.3u).
///
/// RED EVIDENCE. An answer outlives the pixels, which is what stops a bounded
/// cache sending this window round the same decodes for ever — and it is
/// exactly what must not survive the bytes changing under it.
/// `forget_the_picture_in` is the one door where the ledgers about one file
/// are ended together, and its clearing loop walked `tab.preview_panes`: the
/// card's document was not in it, so a card hovering a page kept drawing the
/// picture that used to be in that file.
///
/// The picture *pane*'s own standing answer is ended in the same breath and
/// for the same reason: since §7.1.3u ③ it is read in front of the decode
/// store, so a pane that kept it would draw the old picture of a replaced
/// file and ask for nothing, because what it holds is what it wants.
///
/// MUTATION: leave `peek_pane` out of [`documents_held_mut_in`] and the card
/// keeps its answer; drop the `pane.image` arm from
/// [`forget_standing_answers`] and the pane keeps its raster.
#[test]
fn a_watched_file_moving_clears_the_glance_cards_standing_answer() {
    let file = PathBuf::from(r"D:\proj\shots\one.png");
    let mut pictures = DocumentPictures::default();
    pictures.files.insert(file.clone());
    pictures
        .by_source
        .insert("shots/one.png".to_owned(), MarkdownPicture::Loading);
    let mut card = a_page_holding(pictures);
    // And a picture pane in a tab, standing on the same file.
    let mut pane = PreviewPane {
        image: Some(PreviewImageState::new(file.clone())),
        ..PreviewPane::default()
    };
    if let Some(picture) = pane.image.as_mut() {
        picture.raster = Some(a_held_raster("content-a", (500, 375)));
        picture.native = Some((1024, 768));
    }
    let mut tabs = Vec::new();

    forget_standing_answers(
        documents_held_mut_in(&mut tabs, &mut card).chain(std::iter::once(&mut pane)),
        &file,
    );

    let PreviewDocument::Markdown { pictures, .. } = &card.doc else {
        panic!("the card is still holding a page");
    };
    assert!(
        pictures.files.is_empty() && pictures.by_source.is_empty(),
        "the card kept what it was told about a file that has moved: {pictures:?}"
    );
    let picture = pane.image.as_ref().expect("the pane still shows the file");
    assert!(
        picture.raster.is_none() && picture.native.is_none(),
        "and so did the pane, which would go on drawing the old picture and \
             ask for nothing, because what it holds is what it wants"
    );
    assert_eq!(
        picture.path, file,
        "which file the pane is showing has not changed — only what is in it"
    );
}

/// RED — **a document's own text does not send this window to a share** (route E of the
/// untrusted-path audit, 2026-09-08).
///
/// RED EVIDENCE (2026-09-08). A markdown page is rendered on a *hover*, and rendering it walks
/// every image block and asks for that source's pixels. `link_action` resolved a source into
/// `LinkAction::Preview` on the strength of it being absolute and nothing else, and
/// `request_peek_pixels` asked nothing at all — so `![](\\attacker\share\x.png)` written into
/// any `.md` a reader rests a pointer on was an SMB probe, with the user's credentials, and no
/// click anywhere in it. Before the fix:
///
/// ```text
/// a source this window may not read is a source it never asks for
///   left: ["\\\\attacker\\share\\probe.png"]  right: []
/// ```
///
/// `ask` is `WindowRuntime::resolve_document_pictures`'s closure over `request_peek_pixels`,
/// so "never asked for" is exactly "this closure is not called" — the same seam route A's own
/// test reads one layer up, at the body the card chooses.
///
/// The local sibling in the same document is the control: nothing about an ordinary page
/// changes, and the refused source draws what a picture this window cannot read draws.
///
/// Windows only: `\\server\share\…` is Windows' spelling of another machine. On a
/// filesystem with one root it is a relative file name in the document's folder, and
/// no spelling of a source names another machine there.
///
/// MUTATION: drop the gate from `resolved_link` and the share is asked for again.
#[cfg(windows)]
#[test]
fn a_share_named_by_a_document_is_never_asked_for() {
    let blocks = preview::parse_markdown(
        "![a probe](\\\\attacker\\share\\probe.png)\n\n![a shot](shots/one.png)\n",
    );
    let mut asked: Vec<PathBuf> = Vec::new();
    let pictures = resolve_document_pictures(
        &blocks,
        Some(Path::new(r"D:\proj\README.md")),
        bt_render::Theme::Dark,
        PictureReach::from_the_top(),
        &DocumentPictures::default(),
        &mut |path, _, _| {
            asked.push(path.to_path_buf());
            PagePicture::drawn(MarkdownPicture::Loading)
        },
    );
    assert_eq!(
        asked,
        vec![PathBuf::from(r"D:\proj\shots/one.png")],
        "a source this window may not read is a source it never asks for",
    );
    assert!(
        matches!(
            pictures.by_source.get("\\\\attacker\\share\\probe.png"),
            Some(MarkdownPicture::Failed)
        ),
        "and it draws what a picture this window cannot read draws: {:?}",
        pictures.by_source,
    );
    assert!(
        !pictures
            .files
            .contains(Path::new(r"\\attacker\share\probe.png")),
        "nor is the watch told to follow it",
    );
}

/// RED GATE (user report, 2026-08-28: 「md 预览不渲染图片」) — **a picture's
/// source is resolved against the directory the document was read from**,
/// and the block that carries it is as tall as the picture is drawn.
///
/// The two halves are one rule seen at two moments, the same pair
/// [`a_display_formula_is_as_tall_as_its_picture_and_stands_on_its_source_until_it_comes`]
/// pins: a source resolved against the wrong directory is a picture that is
/// never found, and a block measured from anything but
/// [`markdown_image_extent`] is a screenshot drawn over the paragraph under
/// it.
///
/// MUTATIONS: resolve against the process's working directory instead of the
/// document's and the first assertion goes red; reserve the decode's own
/// height instead of the drawn one and the second does; drop the `.min(1.0)`
/// out of [`markdown_image_extent`] and the badge in the third is blown up
/// to the width of the column.
#[test]
fn a_markdown_image_is_drawn_from_the_documents_own_directory() {
    let blocks = preview::parse_markdown("![a shot](docs/screenshots/one.png)\n");
    let folder = host_path(r"D:\proj");
    let mut asked: Vec<PathBuf> = Vec::new();
    let pictures = resolve_document_pictures(
        &blocks,
        Some(&folder.join("README.md")),
        bt_render::Theme::Dark,
        PictureReach::from_the_top(),
        &DocumentPictures::default(),
        &mut |path, _, _| {
            asked.push(path.to_path_buf());
            PagePicture::drawn(MarkdownPicture::Loading)
        },
    );
    assert_eq!(
        asked,
        vec![folder.join("docs/screenshots/one.png")],
        "the source is relative to the document, not to this process",
    );
    assert_eq!(
        pictures.files,
        [folder.join("docs/screenshots/one.png")]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "and that file is what the watch is told to follow",
    );

    // The block is as tall as the picture is *drawn*: a 3200-wide screenshot
    // brought down to a 400-wide column keeps its shape.
    let metrics = seats::preview_markdown_metrics(1.0);
    let mut never_measured = |_: &[bt_render::PreviewRun], _: f32, _: f32, _: f32| {
        unreachable!("a picture that is in hand asks the shaper nothing")
    };
    let placed = measure_markdown_block(
        &blocks[0],
        &MarkdownBlockIntrinsic::default(),
        400.0,
        metrics,
        PageArt {
            math: &DocumentMath::default(),
            pictures: &one_image("docs/screenshots/one.png", [3200, 2000]),
            theme: bt_render::Theme::Dark,
        },
        &mut never_measured,
    );
    assert_eq!(
        placed.height, 250.0,
        "400 of 3200 is an eighth, and 2000 eighths is 250",
    );

    // And it is never blown up past its own pixels: a small badge stands at
    // its own size in the middle of the column.
    let badge = measure_markdown_block(
        &blocks[0],
        &MarkdownBlockIntrinsic::default(),
        400.0,
        metrics,
        PageArt {
            math: &DocumentMath::default(),
            pictures: &one_image("docs/screenshots/one.png", [96, 20]),
            theme: bt_render::Theme::Dark,
        },
        &mut never_measured,
    );
    assert_eq!(badge.height, 20.0);
    let rendered = build_preview_markdown_body(
        [0.0, 0.0, 400.0, 400.0],
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &[],
            layout: &preview_viewport::Layout::from([MarkdownBlockLayout::solid(20.0)]),
            live: MarkdownLive::default(),
        },
        &bt_render::chrome_palette(),
        PageArt {
            math: &DocumentMath::default(),
            pictures: &one_image("docs/screenshots/one.png", [96, 20]),
            theme: bt_render::Theme::Dark,
        },
    );
    let [drawn] = rendered.body.rasters.as_slice() else {
        panic!("one picture, one raster: {:#?}", rendered.body.rasters);
    };
    let (left, right) = preview::markdown_measure_box([0.0, 0.0, 400.0, 400.0], metrics);
    assert_eq!(
        (drawn.rect[2] - drawn.rect[0], drawn.rect[3] - drawn.rect[1]),
        (96.0, 20.0),
        "its own pixels, never stretched to the column",
    );
    assert_eq!(
        drawn.rect[0] - left,
        right - drawn.rect[2],
        "and the air either side of it is equal",
    );
    assert!(
        rendered.body.paragraphs.is_empty(),
        "with no card under it once the pixels are there: {:?}",
        card_text(&rendered),
    );
}

/// RED — **a page asks for the pictures the reader can see, and not for the
/// five hundred it cannot** (review row R1-8, adversarial review
/// 2026-09-08).
///
/// RED EVIDENCE (2026-09-08), before the reach:
///
/// ```text
/// a page asks for what is near the reader, not for the whole document
///   asked 500, at most 18
/// ```
///
/// The walk went over every image block of the document and asked for every
/// one, so hovering a README with five hundred screenshots in it sent five
/// hundred reads down the one decoration worker and put five hundred decodes
/// into this window's cache — for a card showing the first screenful. The
/// cache's own budget bounds what is *kept*; this bounds what is asked for,
/// and the two are different costs.
///
/// Ten pictures on screen plus [`MARKDOWN_PICTURE_MARGIN`] either side is
/// eighteen: the nine before the first visible one do not exist, so the band
/// runs from the top to eight past the last.
///
/// MUTATIONS: ask regardless of the reach and the count goes back to five
/// hundred; drop the margin and a reader who scrolls one notch is looking at
/// a picture nothing has begun to read.
#[test]
fn a_page_asks_only_for_the_pictures_near_its_viewport() {
    let mut source = String::new();
    for index in 0..500 {
        source.push_str(&format!("![shot {index}](shots/{index}.png)\n\n"));
    }
    let blocks = preview::parse_markdown(&source);
    // A hundred pixels a picture, and a viewport a thousand tall standing at
    // the top: the first ten are on screen.
    let layout: preview_viewport::Layout = blocks
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let mut box_ = MarkdownBlockLayout::solid(100.0);
            box_.top = index as f32 * 100.0;
            box_
        })
        .collect();
    let reach = markdown_picture_reach(&blocks, &layout, 0.0, 1000.0);
    assert_eq!(
        (reach.first, reach.last),
        (0, 9 + MARKDOWN_PICTURE_MARGIN),
        "the ten on screen, and the margin past the last of them",
    );

    let folder = host_path(r"D:\proj");
    let mut asked: Vec<PathBuf> = Vec::new();
    let pictures = resolve_document_pictures(
        &blocks,
        Some(&folder.join("README.md")),
        bt_render::Theme::Dark,
        reach,
        &DocumentPictures::default(),
        &mut |path, _, _| {
            asked.push(path.to_path_buf());
            PagePicture::drawn(MarkdownPicture::Loading)
        },
    );
    assert!(
        asked.len() <= 10 + MARKDOWN_PICTURE_MARGIN,
        "a page asks for what is near the reader, not for the whole document\n  \
             asked {}, at most {}",
        asked.len(),
        10 + MARKDOWN_PICTURE_MARGIN,
    );
    assert_eq!(asked.first(), Some(&folder.join("shots/0.png")));
    assert_eq!(
        pictures.files.len(),
        asked.len(),
        "and the watch follows exactly the files it asked for",
    );

    // And the reader scrolls: the band moves with them, and the pictures
    // behind them are still in it.
    let further = markdown_picture_reach(&blocks, &layout, 20_000.0, 1000.0);
    assert_eq!(
        (further.first, further.last),
        (200 - MARKDOWN_PICTURE_MARGIN, 209 + MARKDOWN_PICTURE_MARGIN),
    );
    assert!(!further.holds(0), "and the top of the document has left it");
}

/// RED GATE (same report; `README.md` and `PRIVACY.md`'s promise) — **a
/// remote picture is never fetched.**
///
/// It is a structural gate and not a behavioural one, and that is the point:
/// `ask` is the only door to a file in [`resolve_document_pictures`], so
/// owning one and counting what reaches it asserts that an `http` source
/// produces **no** read, no request and no lane traffic — rather than that
/// today's code happens to take a different branch. What the reader gets
/// instead is the alt text and the address, as a link this window will open.
///
/// MUTATION: hand a remote source to `ask` and the first assertion goes red
/// with the URL it tried to open as a path.
#[test]
fn a_remote_image_is_never_fetched() {
    let blocks = preview::parse_markdown("![a badge](https://img.example/badge.svg)\n");
    let mut doors = 0usize;
    let pictures = resolve_document_pictures(
        &blocks,
        Some(Path::new(r"D:\proj\README.md")),
        bt_render::Theme::Dark,
        PictureReach::from_the_top(),
        &DocumentPictures::default(),
        &mut |_, _, _| {
            doors += 1;
            PagePicture::drawn(MarkdownPicture::Failed)
        },
    );
    assert_eq!(doors, 0, "nothing asked the disk, or anything else");
    assert!(
        pictures.files.is_empty(),
        "and nothing was handed to the file watch either",
    );
    assert!(matches!(
        pictures.get("https://img.example/badge.svg"),
        Some(MarkdownPicture::Remote(url)) if url == "https://img.example/badge.svg"
    ));

    let metrics = seats::preview_markdown_metrics(1.0);
    let rendered = build_preview_markdown_body(
        [0.0, 0.0, 400.0, 400.0],
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &[],
            layout: &preview_viewport::Layout::from([MarkdownBlockLayout::solid(
                metrics.line_height * 3.0,
            )]),
            live: MarkdownLive::default(),
        },
        &bt_render::chrome_palette(),
        PageArt {
            math: &DocumentMath::default(),
            pictures: &pictures,
            theme: bt_render::Theme::Dark,
        },
    );
    assert!(
        rendered.body.rasters.is_empty(),
        "there are no pixels, because none were fetched",
    );
    let said = card_text(&rendered);
    assert_eq!(said.first().map(String::as_str), Some("a badge"));
    assert!(
        said.last()
            .is_some_and(|note| note.contains("https://img.example/badge.svg")),
        "and the address is printed: {said:?}",
    );
    assert_eq!(
        rendered
            .links
            .iter()
            .map(|site| site.target.as_str())
            .collect::<Vec<_>>(),
        vec!["https://img.example/badge.svg"],
        "as a link, on this window's own terms",
    );
}

/// RED GATE (same report; §7.1.3k ⑬) — **a chip's ground never takes more
/// than a third of the gap beside it.**
///
/// A chip reserves no width of its own: it is a run of text wrapped by the
/// paragraph's own shaper, and its pill is painted round the box the shaper
/// reports. So the only thing between two badges is the space the document
/// wrote, and two pills each taking half of it would meet in the middle and
/// read as one segmented control. A third leaves a third standing, at every
/// size and in every face, without this arithmetic knowing how wide a space
/// is in any of them.
///
/// MUTATION: pad unconditionally and the two grounds below overlap by two
/// pixels; pad by half the gap and they touch exactly.
#[test]
fn a_chips_ground_leaves_a_gap_between_two_badges() {
    let boxed = |run: usize, left: f32, right: f32| bt_render::PreviewRunBox {
        run,
        rect: [left, 100.0, right, 120.0],
        baseline_px: 115.0,
    };
    // Two labels nine pixels apart on one row, with the space between them a
    // run of its own — which is exactly what the shaper reports for a badge
    // row, the space being the join's own between two source lines.
    let boxes = [
        boxed(0, 10.0, 60.0),
        boxed(1, 63.0, 66.0),
        boxed(2, 69.0, 120.0),
    ];
    let first = markdown_chip_ground(boxes[0].rect, &boxes, 1.0);
    let second = markdown_chip_ground(boxes[2].rect, &boxes, 1.0);
    assert!(
        first[2] < second[0],
        "two badges keep a gap: {first:?} then {second:?}",
    );
    assert!(
        (first[2] - 61.0).abs() < 0.01 && (second[0] - 68.0).abs() < 0.01,
        "each takes a third of its own three pixels of space and leaves the \
             rest: {first:?} then {second:?}",
    );
    assert!(
        first[1] > boxes[0].rect[1] && first[3] < boxes[0].rect[3],
        "the pill stands inside the line rather than filling it: {first:?}",
    );
    // With nothing beside it there is nothing to share with.
    let lone = [boxed(0, 10.0, 60.0)];
    let ground = markdown_chip_ground(lone[0].rect, &lone, 1.0);
    assert!(
        (ground[0] - (10.0 - MARKDOWN_CHIP_PADDING_LOGICAL_PX)).abs() < 0.01
            && (ground[2] - (60.0 + MARKDOWN_CHIP_PADDING_LOGICAL_PX)).abs() < 0.01,
        "a chip alone on its row takes the whole padding: {ground:?}",
    );
}

/// RED GATE (same report) — **a picture that will not decode says so where
/// it stands**: the alt text, and one line under it.
///
/// The sentence is not [`i18n::Text::PreviewFailedImageLoad`] and must not
/// become it: nothing failed to *preview* here — the page around the picture
/// is drawn and readable, and one block of it is standing on what it says
/// instead of on what it shows.
///
/// MUTATIONS: draw nothing for a failed picture and the block is a blank box
/// in the middle of the page; draw the alt without the note and the reader
/// is left to guess whether the words are the author's prose.
#[test]
fn an_image_that_will_not_decode_says_so_in_place() {
    let blocks = preview::parse_markdown("![a shot](docs/gone.png)\n");
    let metrics = seats::preview_markdown_metrics(1.0);
    let mut pictures = DocumentPictures::default();
    pictures
        .by_source
        .insert("docs/gone.png".to_owned(), MarkdownPicture::Failed);
    let rendered = build_preview_markdown_body(
        [0.0, 0.0, 400.0, 400.0],
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &[],
            layout: &preview_viewport::Layout::from([MarkdownBlockLayout::solid(
                metrics.line_height * 3.0,
            )]),
            live: MarkdownLive::default(),
        },
        &bt_render::chrome_palette(),
        PageArt {
            math: &DocumentMath::default(),
            pictures: &pictures,
            theme: bt_render::Theme::Dark,
        },
    );
    assert!(rendered.body.rasters.is_empty());
    assert_eq!(
        card_text(&rendered),
        vec![
            "a shot".to_owned(),
            i18n::Text::MarkdownImageUnreadable.text().to_owned(),
        ],
    );
    assert!(
        rendered.links.is_empty(),
        "a file that is not there is not a link either",
    );

    // While the decode is still out there is nothing to tell the reader:
    // "loading" is not a fact about the document, and a card that explains
    // itself and is then replaced by a screenshot is a flash.
    let mut waiting = DocumentPictures::default();
    waiting
        .by_source
        .insert("docs/gone.png".to_owned(), MarkdownPicture::Loading);
    let rendered = build_preview_markdown_body(
        [0.0, 0.0, 400.0, 400.0],
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &[],
            layout: &preview_viewport::Layout::from([MarkdownBlockLayout::solid(
                metrics.line_height * 3.0,
            )]),
            live: MarkdownLive::default(),
        },
        &bt_render::chrome_palette(),
        PageArt {
            math: &DocumentMath::default(),
            pictures: &waiting,
            theme: bt_render::Theme::Dark,
        },
    );
    assert_eq!(card_text(&rendered), vec!["a shot".to_owned()]);
}

/// PIN (same report) — **the card draws its formulas where it reserved them,
/// cuts the ones past its own bottom, and hands what is left to the glass.**
///
/// The reported card showed its headings and left the first `$$` block blank:
/// the picture was in hand, the block was as tall as it, and the raster was
/// filed on the document — where nothing was looking. A card's document does
/// not ride the seats' lane (it has to be drawn *above* the card's own face),
/// so it rides [`bt_render::OverlayLayer::body`], and that lane picked up the
/// body's fills and the body's letters and stopped one channel short of its
/// pictures.
///
/// The last assertion is that seam and nothing else. The two above it are the
/// card's own arithmetic: a formula is drawn at the top its block was placed
/// at, and one placed past the 264-pixel cap is not drawn at all — the same
/// `overflow: hidden` every other block on a card meets.
///
/// MUTATIONS: draw every block whatever its top and the second assertion goes
/// red; hand the layer's icons over without
/// [`bt_render::OverlayLayer::faded_document_rasters`] and the last does,
/// which is the report exactly.
#[test]
fn a_glance_cards_formula_is_drawn_where_it_was_reserved_and_reaches_the_glass() {
    let scale = 1.0;
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(scale);
    // The card's own body box: the probe `file_peek_layer` lays the document
    // out in before it knows how tall the card will be.
    let card = [
        0.0,
        0.0,
        file_peek::body_width(scale),
        file_peek::body_max_height(scale, true),
    ];
    let blocks = [
        preview::MarkdownBlock::Heading {
            level: 1,
            spans: vec![preview::Span::plain("LaTeX 渲染验收语料")],
        },
        preview::MarkdownBlock::Math {
            source: "E = mc^2".to_owned(),
        },
        preview::MarkdownBlock::Math {
            source: "a^2 + b^2 = c^2".to_owned(),
        },
    ];
    let mut math = one_picture(
        "E = mc^2",
        MathMode::Display,
        metrics.font_size,
        (120, 30, 20.0),
    );
    math.insert(
        &PreviewMathKey {
            source: "a^2 + b^2 = c^2".to_owned(),
            mode: MathMode::Display,
            em_milli_px: math_em_milli(metrics.font_size),
            foreground_rgb: [0, 0, 0],
        },
        PreviewMathPicture {
            key: "test:below the cut".to_owned(),
            rgba: Arc::from(vec![0_u8; 120 * 30 * 4].into_boxed_slice()),
            width_px: 120,
            height_px: 30,
            baseline_px: 20.0,
        },
    );
    // Placed by hand so the cut is the thing under test and not the stacking:
    // the first formula well inside the card, the second below its bottom.
    let layout: preview_viewport::Layout = [
        MarkdownBlockLayout::solid(24.0),
        MarkdownBlockLayout {
            width: 120.0,
            top: 40.0,
            ..MarkdownBlockLayout::solid(30.0)
        },
        MarkdownBlockLayout {
            width: 120.0,
            top: card[3] + 10.0,
            ..MarkdownBlockLayout::solid(30.0)
        },
    ]
    .into();
    let rendered = build_preview_markdown_body(
        card,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &[],
            layout: &layout,
            live: MarkdownLive::default(),
        },
        &palette,
        PageArt {
            math: &math,
            pictures: &DocumentPictures::default(),
            theme: bt_render::Theme::Dark,
        },
    );
    let drawn = &rendered.body.rasters;
    assert_eq!(
        drawn.len(),
        1,
        "the formula inside the card is drawn and the one past its bottom is not",
    );
    assert_eq!(
        (drawn[0].rect[1], drawn[0].rect[3]),
        (
            card[1] + metrics.padding_y + 40.0,
            card[1] + metrics.padding_y + 70.0
        ),
        "and it stands at the top its own block was placed at",
    );
    assert!(
        inside(drawn[0].rect, card),
        "inside the card's body: {:?} in {card:?}",
        drawn[0].rect,
    );

    // The seam the report was about: the card hangs its document on an
    // overlay layer, and the pictures in it have to come back out.
    let layer = bt_render::OverlayLayer {
        body: Some(rendered.body.clone()),
        ..bt_render::OverlayLayer::default()
    };
    assert!(
        layer.faded_icons().is_empty(),
        "a document's pictures are not the layer's own marks — which is the \
             whole of the report: the channel they do belong to has to exist",
    );
    assert_eq!(
        layer
            .faded_document_rasters()
            .iter()
            .map(|icon| icon.key.clone())
            .collect::<Vec<_>>(),
        drawn
            .iter()
            .map(|icon| icon.key.clone())
            .collect::<Vec<_>>(),
        "every picture the card's document holds reaches the layer's own channel",
    );
}

/// **`Copy on select` governs the rendered page too** (the switch's own
/// sentence is 「选中即复制」, which is a sentence about a selection).
///
/// MUTATION: add a `travelled` term and double-clicking a word stops
/// copying it on the one surface in this window where that is the whole
/// gesture.
#[test]
fn copy_on_select_governs_a_rendered_page_on_the_terminals_own_terms() {
    assert!(
        preview_copies_on_select(true, true),
        "a drag, and equally a double click: both left bytes standing",
    );
    assert!(
        !preview_copies_on_select(true, false),
        "a click that selected nothing has nothing to write, which is the \
             terminal's `!single_click` guard said in this surface's own terms",
    );
    assert!(
        !preview_copies_on_select(false, true),
        "with the switch off, only Ctrl+C and the menu row write",
    );
}

/// **A press on a Chinese character lands on that character** (user report,
/// 2026-09-11) — the drawn grid read backwards, cluster by cluster.
///
/// An ideograph owns two cells and a caret may not stand between them, so
/// the seam that decides which side of it a press belongs to is the
/// character's own middle. Rounding the pointer to a whole cell first, which
/// is what this did, threw that away: the left half of a character rounds
/// into its *second* cell, and the second cell of a wide cluster rounds
/// forward out of it — so a press aimed at an ideograph seated the caret
/// after it, one whole character from where the pointer was.
///
/// MUTATION: round the pointer to a cell before asking, and every press on
/// the left half of an ideograph lands after it.
#[test]
fn a_press_on_a_wide_character_lands_on_the_character_it_is_over() {
    // Eight pixels to the cell, so an ideograph is drawn sixteen wide.
    let source = source_block(0, 0, "中文 ab");
    let box_of_block = [100.0, 40.0, 500.0, 60.0];
    let at = |x: f32| markdown_source_offset_at(&source, box_of_block, x, 44.0);
    // 「中」 stands in columns 0 and 1 — pixels 100 to 116 — and 「文」 in 2
    // and 3, which is 116 to 132.
    assert_eq!(at(100.0), 0, "its left edge is the seam in front of it");
    assert_eq!(at(104.0), 0, "a quarter in is still in front of it");
    assert_eq!(
        at(108.0),
        0,
        "and its middle is the character the pointer is on, not the next one",
    );
    assert_eq!(at(112.0), 3, "past the middle is the seam behind it");
    assert_eq!(at(116.0), 3, "which is where the next character begins");
    assert_eq!(at(124.0), 3, "the middle of 「文」 is 「文」's own byte");
    assert_eq!(at(128.0), 6, "and past its middle is the space after it");
    // A one-cell letter keeps the rule it always had: its own middle.
    assert_eq!(
        at(136.0),
        6,
        "the space's own middle belongs to the seam in front of it",
    );
    assert_eq!(at(140.0), 7, "`a` begins at column 5");
    assert_eq!(at(146.0), 8, "and past its middle is between `a` and `b`");
    // And every seam the caret can be at is a seam a press can reach.
    for (byte, column) in [(0usize, 0usize), (3, 2), (6, 4), (7, 5), (8, 6), (9, 7)] {
        assert_eq!(
            preview_edit::column_of(&source.text, byte),
            column,
            "the fixture's own columns",
        );
        assert_eq!(
            at(100.0 + column as f32 * 8.0),
            byte,
            "a press on the seam at column {column} names byte {byte}",
        );
    }
}

/// PIN (user report, 2026-08-13 — **the escape**) — nothing a preview body
/// produces may be drawn outside the preview's own rectangle.
///
/// The report: opening a document holding a 250-character fence line and a
/// table of very long cells put grey bands and stray words *across the whole
/// window* — over the file tree, over the terminal. A preview that draws on
/// its neighbours is a different class of defect from a preview that draws
/// its own content badly, and it has exactly two causes, both asserted here:
///
/// ① a rectangle that the renderer's crop does not actually bound —
///    `PreviewBody::clip` is the one gate, and every quad and every
///    paragraph has to go through it;
/// ② a rectangle that is **inverted or non-finite**, which no crop can save:
///    the pass runs in whole-surface coordinates with no scissor, so
///    `right < left` is a box the rasteriser fills across the surface and a
///    `NaN` is a box with no edges. `bt_render::crop_to` is the gate for
///    that one, and this test's job is to prove the layout never asks it to
///    fire — a rectangle silently dropped is content silently missing.
///
/// Run against `stress.md`, the document the report was made with, at a pane
/// deliberately far narrower than its widest block, and at every horizontal
/// scroll offset the body can reach — because the offset is precisely what
/// moves a wide block's rectangle to a negative x.
///
/// MUTATION: drop the `.max(body.clip[..])` crop in
/// `bt_render::shape_preview_body`, or return the rect unclipped from
/// `crop_to`, and the containment assertion goes red — which is the reported
/// screenshot, in one number.
#[test]
fn nothing_a_preview_body_produces_is_drawn_outside_the_preview() {
    let palette = bt_render::chrome_palette();
    let source = include_str!("../../../tests/assets/preview-samples/stress.md");
    let blocks = preview::parse_markdown(source);
    // A narrow pane, so every wide block overruns it and the crop is the only
    // thing standing between the document and the panes beside it.
    let body = [480.0, 120.0, 800.0, 620.0];
    let metrics = seats::preview_markdown_metrics(1.0);

    // A stand-in measurer, eight pixels a character — the same monospace
    // grid the ruling test uses, so the widths here are predictable and the
    // wide blocks are genuinely wider than the pane.
    let cell = 8.0_f32;
    let mut layout = Vec::with_capacity(blocks.len());
    let mut top = 0.0_f32;
    let mut previous_bottom = 0.0_f32;
    let mut previous_block: Option<&preview::MarkdownBlock> = None;
    for block in &blocks {
        let mut measured = match block {
            preview::MarkdownBlock::Code { text, .. } => MarkdownBlockLayout {
                width: markdown_fence_width(text, metrics, |runs| {
                    runs.iter()
                        .map(|run| run.text.chars().count())
                        .sum::<usize>() as f32
                        * cell
                }),
                ..MarkdownBlockLayout::solid(
                    metrics.code_border * 2.0
                        + metrics.code_padding_y * 2.0
                        + metrics.line_height * text.lines().count().max(1) as f32,
                )
            },
            preview::MarkdownBlock::Table { rows, .. } => {
                let columns = markdown_table_columns(rows, metrics, |cell_spans, _| {
                    cell_spans
                        .iter()
                        .map(|span| span.text.chars().count())
                        .sum::<usize>() as f32
                        * cell
                });
                let width = columns.iter().sum::<f32>() + metrics.table_border;
                let heights =
                    vec![
                        metrics.line_height + metrics.table_border + metrics.table_padding_y * 2.0;
                        rows.len()
                    ];
                MarkdownBlockLayout {
                    columns,
                    width,
                    ..MarkdownBlockLayout::rows(heights, metrics.table_border)
                }
            }
            preview::MarkdownBlock::List { items, .. } => {
                MarkdownBlockLayout::rows(vec![metrics.line_height; items.len()], 0.0)
            }
            preview::MarkdownBlock::Quote(lines) => MarkdownBlockLayout::rows(
                vec![metrics.line_height; lines.len()],
                metrics.quote_padding_y * 2.0,
            ),
            preview::MarkdownBlock::Rule => MarkdownBlockLayout::solid(metrics.rule_thickness),
            _ => MarkdownBlockLayout::solid(metrics.line_height * 2.0),
        };
        // Updated 2026-08-16 with the metrics: margins are a top and a
        // bottom now, and the block before this one is what answers the two
        // `:first-child` rules. See `preview::markdown_block_margins`.
        let (margin_top, margin_bottom) =
            preview::markdown_block_margins(block, previous_block, metrics);
        top += previous_bottom.max(margin_top);
        measured.top = top;
        top += measured.height;
        previous_bottom = margin_bottom;
        previous_block = Some(block);
        layout.push(measured);
    }
    let layout: preview_viewport::Layout = layout.into();
    let widest = layout.iter().map(|b| b.width).fold(0.0_f32, f32::max);
    assert!(
        widest > (body[2] - body[0]) * 2.0,
        "the fixture must overrun the pane by a wide margin, or this proves \
             nothing: {widest} against {}",
        body[2] - body[0]
    );

    let document = PreviewDocument::Markdown {
        intrinsic: Vec::new(),
        blocks: blocks.clone(),
        ranges: Vec::new(),
        maps: Vec::new(),
        source: SourceBlocks::default(),
        layout: layout.clone(),
        math: DocumentMath::default(),
        pictures: DocumentPictures::default(),
        wrap: Arc::default(),
    };
    let max = preview_document_max_scroll(&document, body, 1.0, cell, 0.0, 0);
    assert_eq!(max[0], 0.0, "the page has no horizontal axis of its own");
    assert!(max[1] > 0.0, "and it certainly overruns downwards");
    // The horizontal offset that could push a rectangle negative now belongs
    // to the *block*, so it is the axis this sweeps — every block gets the
    // page's own width as an offset, which is far past any of their ends and
    // is exactly the arithmetic the crop has to survive.
    let overrun = widest;
    for x in [0.0, overrun / 2.0, overrun] {
        let offsets = vec![x; layout.len()];
        for y in [0.0, max[1] / 2.0, max[1]] {
            let built = markdown_body(
                body,
                metrics,
                [0.0, y],
                rested_bars(&offsets),
                (&blocks, &layout),
                &palette,
            );
            assert_eq!(built.clip, body, "the body's one gate is the pane's box");
            let finite = |rect: [f32; 4]| rect.iter().all(|value| value.is_finite());
            for quad in &built.quads {
                assert!(
                    finite(quad.rect),
                    "a non-finite fill at scroll {x},{y}: {:?}",
                    quad.rect
                );
                assert!(
                    quad.rect[2] >= quad.rect[0] && quad.rect[3] >= quad.rect[1],
                    "an inverted fill at scroll {x},{y}: {:?} — the rasteriser \
                         fills one of these across the whole surface",
                    quad.rect
                );
                assert!(
                    bt_render::crop_to(quad.rect, built.clip)
                        .is_none_or(|drawn| inside(drawn, built.clip)),
                    "a fill that survives the crop outside the pane: {:?}",
                    quad.rect
                );
            }
            for paragraph in &built.paragraphs {
                assert!(
                    finite(paragraph.rect),
                    "a non-finite paragraph at scroll {x},{y}: {:?}",
                    paragraph.rect
                );
                assert!(
                    paragraph.rect[2] >= paragraph.rect[0]
                        && paragraph.rect[3] >= paragraph.rect[1],
                    "an inverted paragraph at scroll {x},{y}: {:?}",
                    paragraph.rect
                );
                assert!(
                    bt_render::crop_to(paragraph.rect, built.clip)
                        .is_none_or(|drawn| inside(drawn, built.clip)),
                    "a paragraph that survives the crop outside the pane: {:?}",
                    paragraph.rect
                );
            }
            // And the same three questions of every scrolling block, whose
            // rectangles are the ones the offset actually moves. Their gate
            // is the block's clip intersected with the body's, which is what
            // `bt_render` crops them to.
            for block in &built.blocks {
                assert!(finite(block.clip));
                assert!(block.clip[2] >= block.clip[0] && block.clip[3] >= block.clip[1]);
                let window = bt_render::crop_to(block.clip, built.clip);
                for quad in &block.quads {
                    assert!(
                        finite(quad.rect)
                            && quad.rect[2] >= quad.rect[0]
                            && quad.rect[3] >= quad.rect[1],
                        "a block fill at offset {x}: {:?}",
                        quad.rect
                    );
                    assert!(
                        window
                            .and_then(|window| bt_render::crop_to(quad.rect, window))
                            .is_none_or(|drawn| inside(drawn, built.clip)),
                        "a block fill that survives the crop outside the pane: {:?}",
                        quad.rect
                    );
                }
                for paragraph in &block.paragraphs {
                    assert!(
                        finite(paragraph.rect)
                            && paragraph.rect[2] >= paragraph.rect[0]
                            && paragraph.rect[3] >= paragraph.rect[1],
                        "a block paragraph at offset {x}: {:?}",
                        paragraph.rect
                    );
                    assert!(
                        window
                            .and_then(|window| bt_render::crop_to(paragraph.rect, window))
                            .is_none_or(|drawn| inside(drawn, built.clip)),
                        "a block paragraph that survives the crop outside the pane: {:?}",
                        paragraph.rect
                    );
                }
            }
        }
    }
}

/// **A paragraph under the caret keeps the body face and shows its marks**
/// (§7.1.3w, owner's ruling 2026-09-11 「就按 Obsidian 那样做」).
///
/// The ruling reversed §7.1.3q's one exception: a paragraph you click into
/// must not change typeface. What comes back when the caret arrives is the
/// **marks** — the stars, the brackets, the `- ` — drawn as the characters
/// they are, at the places they are in the file; the face is the face the
/// page was read in, and the block is soft-wrapped by the ordinary paragraph
/// shaper rather than folded on a grid.
///
/// The fixtures are the three every text ticket here carries: ASCII, CJK and
/// a line that changes script.
///
/// MUTATION ①: set the prose block's runs `mono` and a reader who clicks
/// into a paragraph watches the whole paragraph change typeface, which is
/// exactly the report. MUTATION ②: draw the rendered arm as well and the
/// paragraph is set twice, over itself. MUTATION ③: give the paragraph
/// `wrap: false` and a long line runs off the right of a page that has no
/// horizontal axis.
#[test]
fn a_paragraph_under_the_caret_keeps_the_body_face_and_shows_its_marks() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 400.0, 400.0];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    for marked in [
        "a **bold** word and a [link](x)",
        "**预览**窗格提示",
        "abc **中文** def",
    ] {
        let blocks = prose(&["first", "middle", "last"]);
        let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
        let source = prose_caret_block(1, 6, marked);
        let layout = lay_markdown_out(
            &blocks,
            &intrinsic,
            &SourceBlocks::from(source.clone()),
            400.0,
            metrics,
            art,
            &mut shaper,
        );
        let built = build_preview_markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(&[]),
            MarkdownPage {
                blocks: &blocks,
                intrinsic: &intrinsic,
                layout: &layout,
                live: MarkdownLive {
                    source: &SourceBlocks::from(source.clone()),
                    caret: None,
                },
            },
            &palette,
            art,
        );
        let words: Vec<String> = built
            .body
            .paragraphs
            .iter()
            .map(|paragraph| {
                paragraph
                    .runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>()
            })
            .collect();
        assert_eq!(
            words,
            ["first", marked, "last"],
            "the caret's block is its own bytes, marks and all, and is set once",
        );
        let block = &built.body.paragraphs[1];
        assert!(
            block.runs.iter().all(|run| !run.mono),
            "{marked:?} keeps the body face it was read in",
        );
        assert!(
            (block.font_size_px - metrics.font_size).abs() < f32::EPSILON,
            "and the paragraph's own size: {} against {}",
            block.font_size_px,
            metrics.font_size,
        );
        assert!(block.wrap, "and it soft-wraps like any other paragraph");
        assert_eq!(
            block.cell_advance, None,
            "on no grid: the grid is the other face's",
        );
        // No chrome: the bytes that would have made it are on the screen.
        assert!(
            built.body.quads.is_empty(),
            "{marked:?} draws no bar, no bullet and no rule: {:#?}",
            built.body.quads,
        );
    }
}

/// **A heading under the caret keeps its size and shows its hashes**
/// (§7.1.3w).
///
/// The one prose kind whose face is not the paragraph's: a heading is set at
/// its own size and weight, and the `##` stands inside it at that size,
/// because a heading that shrank to body size when it was clicked into would
/// be changing typeface exactly as a paragraph turning monospace would.
///
/// MUTATION: set every prose block at `metrics.font_size` and clicking into
/// a title makes the title the size of the prose under it.
#[test]
fn a_heading_under_the_caret_keeps_its_size_and_shows_its_hashes() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 400.0, 400.0];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let blocks = vec![preview::MarkdownBlock::Heading {
        level: 2,
        spans: vec![preview::Span::plain("标题 Title")],
    }];
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let heading = "## 标题 Title";
    let source = MarkdownCaretBlock::Prose(MarkdownProseBlock {
        index: 0,
        range: 0..heading.len() + 1,
        lines: prose_source_lines(heading),
        text: heading.to_owned(),
        heading: true,
        font_size: metrics.heading_font(2),
        line_height: metrics.heading_line_height(2),
    });
    let layout = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        400.0,
        metrics,
        art,
        &mut shaper,
    );
    let built = build_preview_markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &intrinsic,
            layout: &layout,
            live: MarkdownLive {
                source: &SourceBlocks::from(source.clone()),
                caret: None,
            },
        },
        &palette,
        art,
    );
    let [line] = built.body.paragraphs.as_slice() else {
        panic!("one line, set once: {:#?}", built.body.paragraphs);
    };
    assert_eq!(
        line.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        heading,
        "the hashes are on the screen",
    );
    assert!(
        (line.font_size_px - metrics.heading_font(2)).abs() < f32::EPSILON,
        "at the heading's own size",
    );
    assert!(
        line.runs.iter().all(|run| run.bold && !run.mono),
        "and in the heading's own weight, in the body family",
    );
    assert!(
        built.body.quads.is_empty(),
        "and the heading's hairline stands down with the rest of the chrome",
    );
}

/// **A code fence, a table and a display formula under the caret are still
/// the monospace source block** (§7.1.3w).
///
/// The ruling's other half, and the reason for it: those are the blocks
/// whose *alignment* is part of what they say, so monospace is the honest
/// face to edit them in. Prose is the four kinds that carry sentences.
///
/// MUTATION: answer prose for a fence and a reader editing a table watches
/// its columns stop lining up under their own pipes.
#[test]
fn a_code_fence_under_the_caret_is_still_the_monospace_source_block() {
    let mono = [
        preview::MarkdownBlock::Code {
            lang: Some("rust".to_owned()),
            text: "fn main() {}\n".to_owned(),
        },
        preview::MarkdownBlock::Table {
            rows: vec![vec![vec![preview::Span::plain("a")]]],
            alignments: Vec::new(),
        },
        preview::MarkdownBlock::Math {
            source: "x^2".to_owned(),
        },
        preview::MarkdownBlock::Rule,
    ];
    for block in &mono {
        assert_eq!(
            markdown_prose_face(block),
            None,
            "{block:?} is drawn in the monospace source face",
        );
    }
    assert_eq!(
        markdown_prose_face(&preview::MarkdownBlock::Paragraph(vec![
            preview::Span::plain("p")
        ])),
        Some(None),
        "a paragraph is prose at the paragraph's size",
    );
    assert_eq!(
        markdown_prose_face(&preview::MarkdownBlock::Quote(vec![vec![
            preview::Span::plain("q")
        ]])),
        Some(None),
        "and so is a quote",
    );
    assert_eq!(
        markdown_prose_face(&preview::MarkdownBlock::List {
            ordered: None,
            items: vec![vec![preview::Span::plain("i")]],
        }),
        Some(None),
        "and a list",
    );
    assert_eq!(
        markdown_prose_face(&preview::MarkdownBlock::Heading {
            level: 3,
            spans: vec![preview::Span::plain("h")],
        }),
        Some(Some(3)),
        "and a heading, at its own level's size",
    );
}

/// **The empty page and a gap take the prose face** (§7.1.3w).
///
/// A caret between two blocks stands on an empty line of a document whose
/// prose is set in the body face, so the bar it is drawn as is a body line
/// tall. A caret a monospace line tall standing between two paragraphs would
/// be announcing a face nothing on that page is set in.
///
/// MUTATION: hand the gap the text face's line height and the caret between
/// two paragraphs is visibly shorter than the words either side of it.
#[test]
fn the_empty_page_and_a_gap_take_the_prose_face() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 400.0, 400.0];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let blocks = prose(&["first", "last"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let layout = lay_markdown_out(
        &blocks,
        &intrinsic,
        &NO_SOURCE_BLOCKS,
        400.0,
        metrics,
        art,
        &mut shaper,
    );
    let caret = MarkdownCaretPaint {
        seat: MarkdownCaretSeat::Gap {
            after: Some(0),
            line_height: metrics.line_height,
        },
        lit: true,
        selection: 0..0,
        band: 0..0,
        caret_width: 2.0,
        preedit: None,
    };
    let built = build_preview_markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &intrinsic,
            layout: &layout,
            live: MarkdownLive {
                source: &NO_SOURCE_BLOCKS,
                caret: Some(&caret),
            },
        },
        &palette,
        art,
    );
    let [bar] = built.body.quads.as_slice() else {
        panic!("one caret and nothing else: {:#?}", built.body.quads);
    };
    assert!(
        (bar.rect[3] - bar.rect[1] - metrics.line_height).abs() < f32::EPSILON,
        "the gap's caret is a body line tall: {bar:?}",
    );
    // And the page with nothing on it at all is the same gap with no block
    // in front of it — it still takes a caret from a press anywhere in it.
    let empty = PreviewDocument::Markdown {
        blocks: Vec::new(),
        ranges: Vec::new(),
        maps: Vec::new(),
        source: SourceBlocks::default(),
        intrinsic: Vec::new(),
        layout: preview_viewport::Layout::default(),
        math: DocumentMath::default(),
        pictures: DocumentPictures::default(),
        wrap: Arc::default(),
    };
    assert_eq!(
        markdown_empty_page_offset(&empty, body, 200.0, 200.0),
        Some(0)
    );
    // And the face that height comes from is the page's own, which is the
    // decision this test is really about: the seat is cut in
    // `preview_markdown_caret` and nowhere else.
    let seat = method_body("Runtime", "preview_markdown_caret");
    assert!(
        seat.contains("preview_markdown_metrics(scale).line_height"),
        "the gap's empty line is a body line, not a source line",
    );
}

/// **One geometry answers the caret, the candidate box, the press, the rows
/// and the scroll in a prose block** (§7.1.3u, §7.1.3w).
///
/// Said about the source rather than about a rectangle, because what is
/// being held is not a number but a *shape*: every one of the five reads the
/// pane's own [`PreviewPane::md_prose`], which is what the pass that drew the
/// block asked the shaper that drew it. A second derivation of any of them is
/// a caret standing beside the character it edits, and this window has paid
/// for that twice (§7.1.3q's last paragraph, `4381300`).
#[test]
fn the_carets_prose_block_is_read_in_one_geometry() {
    let body = |name: &str| method_body("Runtime", name);
    for (reader, what) in [
        ("preview_md_file_offset_at", "the press"),
        ("live_markdown_ime_cursor_area", "the candidate box"),
        ("move_preview_caret", "Up and Down"),
        ("reveal_live_markdown_caret", "the scroll"),
    ] {
        assert!(
            body(reader).contains("md_prose"),
            "{what} reads the prose block's one geometry",
        );
    }
    // And the one pass that fills it is the one that drew the block: the
    // paragraphs it measures come from the painter's own function.
    assert!(
        body("preview_prose_geometry").contains("markdown_prose_paragraphs("),
        "the geometry is measured off the very paragraphs that are drawn",
    );
}

/// **The same composition, in the face a fence and a table wear** (§7.1.3w)
/// — the block's own cells, which is where its caret is counted.
///
/// The monospace block needs no shaper for any of it: the letters go on the
/// grid, the row they land in is drawn as two runs with the composition's
/// own width of clear space between them, and the rule under them is a
/// rectangle a whole number of cells wide. That is
/// [`build_preview_text_body`]'s machine over one block instead of one
/// document.
///
/// MUTATION ①: draw the row whole and paint the letters over it — the split
/// assertion goes red, and the composition and the rest of the line occupy
/// the same cells with neither readable (the 2026-08-13 capture, one face
/// over).
/// MUTATION ②: leave the caret at the block's own column and the last goes
/// red — the bar stands in front of the letters being composed instead of
/// inside them.
#[test]
fn a_composition_is_drawn_at_the_caret_in_a_monospace_block() {
    let palette = bt_render::chrome_palette();
    let source = source_block(0, 0, "| 天 | b |");
    let box_of_block = [10.0, 100.0, 410.0, 120.0];
    let column = preview_edit::column_of(&source.text, "| 天 ".len());
    let draw = |preedit: Option<MarkdownPreedit>| {
        let mut quads = Vec::new();
        let mut paragraphs = Vec::new();
        push_markdown_source_block(
            (&mut quads, &mut paragraphs),
            &source,
            Some(&MarkdownCaretPaint {
                seat: MarkdownCaretSeat::Source {
                    block: source.index,
                    line: 0,
                    column,
                },
                lit: true,
                selection: 0..0,
                band: 0..0,
                caret_width: 2.0,
                preedit,
            }),
            &highlight::Highlighting::plain(),
            box_of_block,
            [0.0, 0.0, 1000.0, 1000.0],
            &palette,
        );
        (quads, paragraphs)
    };
    let (bare, plain) = draw(None);
    assert_eq!(plain.len(), 1, "one row, drawn whole");
    assert_eq!(bare.len(), 1, "and one caret");

    let (quads, paragraphs) = draw(Some(MarkdownPreedit {
        text: "nikan".to_owned(),
        caret_byte: 2,
    }));
    assert_eq!(
        paragraphs.len(),
        3,
        "the row is cut in two and the composition is the third paragraph",
    );
    assert_eq!(
        paragraphs[2].runs[0].text, "nikan",
        "the letters being composed, set on the block's own cells",
    );
    assert_eq!(
        paragraphs[2].rect[0],
        markdown_source_cell(&source, box_of_block, 0, column)[0],
        "at the caret's own cell",
    );
    assert_eq!(
        paragraphs[1].rect[0] - paragraphs[0].rect[0],
        source.advance * (column + 5) as f32,
        "and the rest of the line is pushed along by the composition's width \
             rather than drawn under it",
    );
    let [rule, caret] = quads.as_slice() else {
        panic!("the composition's rule and the caret inside it: {quads:#?}");
    };
    assert_eq!(
        [rule.rect[0], rule.rect[2]],
        [
            markdown_source_cell(&source, box_of_block, 0, column)[0],
            markdown_source_cell(&source, box_of_block, 0, column + 5)[0],
        ],
        "the rule spans the composition and says it is not in the file yet",
    );
    assert_eq!(
        caret.rect[0],
        markdown_source_cell(&source, box_of_block, 0, column + 2)[0],
        "and the caret stands inside the letters, where the method put it",
    );
}

/// **A composition is never written into the buffer** (§7.1.3q: a
/// composition is drawn, not typed).
///
/// The rule every text field in this window already keeps
/// (`TextField::set_preedit`), asked of the two faces of a rendered page:
/// what the reader can see gains the letters and the block's own bytes do
/// not, so an Escape that cancels a composition leaves the document exactly
/// as it was with nothing to un-type.
///
/// MUTATION: insert the pre-edit through `insert_into_preview` on the
/// `Preedit` arm and the source pin goes red — the file would hold text
/// nobody typed, and the dirty dot would come on for it.
#[test]
fn a_composition_is_never_written_into_the_buffer() {
    let palette = bt_render::chrome_palette();
    let text = "我们是天下第一好";
    let prose = MarkdownProseBlock {
        index: 0,
        range: 0..text.len() + 1,
        lines: prose_source_lines(text),
        text: text.to_owned(),
        heading: false,
        font_size: 14.0,
        line_height: 20.0,
    };
    let preedit = MarkdownPreedit {
        text: "nikan".to_owned(),
        caret_byte: 5,
    };
    let before = prose.clone();
    let _ = markdown_prose_paragraphs(
        &prose,
        [10.0, 100.0, 210.0, 120.0],
        &[20.0],
        Some(("我们是".len(), &preedit)),
        &palette,
    );
    assert_eq!(prose, before, "drawing a composition changes no block");

    let source = source_block(0, 0, "| 天 | b |");
    let kept = source.clone();
    let (mut quads, mut paragraphs) = (Vec::new(), Vec::new());
    push_markdown_source_block(
        (&mut quads, &mut paragraphs),
        &source,
        Some(&MarkdownCaretPaint {
            seat: MarkdownCaretSeat::Source {
                block: source.index,
                line: 0,
                column: 2,
            },
            lit: true,
            selection: 0..0,
            band: 0..0,
            caret_width: 2.0,
            preedit: Some(preedit),
        }),
        &highlight::Highlighting::plain(),
        [10.0, 100.0, 410.0, 120.0],
        [0.0, 0.0, 1000.0, 1000.0],
        &palette,
    );
    assert_eq!(source, kept, "on either face");

    // And the door the letters actually arrive at puts them in the window's
    // own `preedit` and never through the buffer's edit door.
    let door = method_body("Runtime", "preview_ime");
    let preedit_arm = door
        .split("Ime::Commit")
        .next()
        .expect("the pre-edit arm stands above the commit");
    assert!(
        !preedit_arm.contains("insert_into_preview"),
        "a pre-edit is drawn and never inserted",
    );
}

/// **A gap and a page with nothing on it draw a composition too** (§7.1.3q).
///
/// The gap is the one place on the page where a composition displaces
/// nothing: it holds no bytes, so the letters stand at the column a line
/// starts in and the document does not move. An empty page is the same
/// place with no block in front of it.
///
/// MUTATION: draw the bare caret bar as well as the letters and the second
/// assertion goes red — two carets on one empty line, one of them at the
/// margin and one inside the composition.
#[test]
fn a_gap_and_the_empty_page_draw_a_composition_too() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 400.0, 400.0];
    let (left, _) = preview::markdown_measure_box(body, metrics);
    let blocks = prose(&["first", "second"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let layout = lay_markdown_out(
        &blocks,
        &intrinsic,
        &NO_SOURCE_BLOCKS,
        400.0,
        metrics,
        art,
        &mut shaper,
    );
    let page = |caret: &MarkdownCaretPaint| {
        build_preview_markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(&[]),
            MarkdownPage {
                blocks: &blocks,
                intrinsic: &intrinsic,
                layout: &layout,
                live: MarkdownLive {
                    source: &NO_SOURCE_BLOCKS,
                    caret: Some(caret),
                },
            },
            &palette,
            art,
        )
    };
    let composing = MarkdownCaretPaint {
        seat: MarkdownCaretSeat::Gap {
            after: Some(0),
            line_height: 20.0,
        },
        lit: true,
        selection: 0..0,
        band: 0..0,
        caret_width: 2.0,
        preedit: Some(MarkdownPreedit {
            text: "nikan".to_owned(),
            caret_byte: 5,
        }),
    };
    let built = page(&composing);
    let composed = built
        .body
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.runs.iter().any(|run| run.text == "nikan"))
        .expect("the letters being composed are on the empty line");
    assert_eq!(
        [composed.rect[0], composed.rect[1]],
        [left, metrics.padding_y + layout[0].top + layout[0].height],
        "at the column a line starts in, directly under the block the caret \
             has just left",
    );
    assert!(
        built.body.quads.is_empty(),
        "and the bar is the composition's, struck from the shaper's own \
             seams rather than twice: {:#?}",
        built.body.quads,
    );
    let laid_again = lay_markdown_out(
        &blocks,
        &intrinsic,
        &NO_SOURCE_BLOCKS,
        400.0,
        metrics,
        art,
        &mut shaper,
    );
    assert_eq!(layout, laid_again, "and the page did not move to make room");

    // A page with nothing on it is the same gap with no block in front of
    // it: the caret is byte zero and the letters stand at the top.
    let ahead = MarkdownCaretPaint {
        seat: MarkdownCaretSeat::Gap {
            after: None,
            line_height: 20.0,
        },
        ..composing
    };
    let built = page(&ahead);
    let composed = built
        .body
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.runs.iter().any(|run| run.text == "nikan"))
        .expect("a page in front of every block composes at the top of it");
    assert_eq!(composed.rect[1], metrics.padding_y);
}

/// RED (preview report 2026-09-23, A) — **the source block is banded from the
/// band it is handed, not from the caret's own selection.**
///
/// The paint half of the fix. A rendered selection crossing the source block
/// reaches the painter as [`MarkdownCaretPaint::band`] while the caret's own
/// range stays collapsed where it stands; the monospace block — a table under
/// the caret, as in the report's appendix table — has to band its rows from the
/// first and not from the second. The rows are the block's own two lines, each
/// banded whole because the band runs past both ends of the block.
///
/// MUTATION: cut `caret.selection` in `push_markdown_source_block` (the old
/// painter) and the block draws its caret and no band at all.
#[test]
fn a_source_block_bands_its_rows_from_the_band_it_is_handed() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 400.0, 400.0];
    let (left, _) = preview::markdown_measure_box(body, metrics);
    let blocks = prose(&["first", "middle", "last"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let source = mono_caret_block(1, 6, "| a |\n| bb |");
    let mono = source.mono().expect("a table wears the mono face");
    let layout = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        400.0,
        metrics,
        art,
        &mut shaper,
    );
    let caret = MarkdownCaretPaint {
        seat: MarkdownCaretSeat::Source {
            block: source.index(),
            line: 0,
            column: 0,
        },
        lit: true,
        selection: 6..6,
        band: 0..40,
        caret_width: 2.0,
        preedit: None,
    };
    let built = build_preview_markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &intrinsic,
            layout: &layout,
            live: MarkdownLive {
                source: &SourceBlocks::from(source.clone()),
                caret: Some(&caret),
            },
        },
        &palette,
        art,
    );
    let bands: Vec<[f32; 4]> = built
        .body
        .quads
        .iter()
        .filter(|quad| quad.color == palette.preview_selection)
        .map(|quad| quad.rect)
        .collect();
    let top = metrics.padding_y + layout[1].top;
    assert_eq!(bands.len(), 2, "one band per row of the block: {bands:?}");
    for (row, (band, columns)) in bands.iter().zip([5.0, 6.0]).enumerate() {
        let row_top = top + mono.line_height * row as f32;
        assert_eq!(
            [band[0], band[1], band[3]],
            [left, row_top, row_top + mono.line_height],
            "row {row} is banded from its first column, on its own row",
        );
        assert!(
            band[2] >= left + mono.advance * columns,
            "and to the end of its text at least: {band:?}",
        );
    }
}

/// RED (owner's ruling 2026-09-23, B) — **every block in the source set is
/// drawn from its own bytes and banded, and only the caret's block carries the
/// caret.**
///
/// The painter's half of the ruling. A selection from the first block into the
/// third draws both as their source rows with the band across each, and a
/// block between them that is not in the set — as a table the selection only
/// swept is not — stays rendered. The caret stands in exactly one block: the
/// third, where the selection's head is.
///
/// MUTATION: draw the caret from `MarkdownCaretSeat::Source` without matching
/// its block (the one-block painter) and the first block draws a second caret
/// at the same line and column.
#[test]
fn every_block_in_the_source_set_is_drawn_as_source_and_only_one_holds_the_caret() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 400.0, 400.0];
    let blocks = prose(&["first", "middle", "last"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let set = SourceBlocks::new(vec![
        mono_caret_block(0, 0, "| one |"),
        mono_caret_block(2, 20, "| three |"),
    ]);
    let layout = lay_markdown_out(&blocks, &intrinsic, &set, 400.0, metrics, art, &mut shaper);
    let caret = MarkdownCaretPaint {
        seat: MarkdownCaretSeat::Source {
            block: 2,
            line: 0,
            column: 3,
        },
        lit: true,
        selection: 2..23,
        band: 2..23,
        caret_width: 2.0,
        preedit: None,
    };
    let built = build_preview_markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &intrinsic,
            layout: &layout,
            live: MarkdownLive {
                source: &set,
                caret: Some(&caret),
            },
        },
        &palette,
        art,
    );
    let texts: Vec<String> = built
        .body
        .paragraphs
        .iter()
        .map(|paragraph| paragraph.runs.iter().map(|run| run.text.as_str()).collect())
        .collect();
    assert!(
        texts.iter().any(|text| text == "| one |") && texts.iter().any(|text| text == "| three |"),
        "both source blocks are drawn as their own bytes: {texts:?}",
    );
    assert!(
        texts.iter().any(|text| text.contains("middle")),
        "and the block between them is drawn rendered: {texts:?}",
    );
    let top_of = |index: usize| metrics.padding_y + layout[index].top;
    let bands: Vec<[f32; 4]> = built
        .body
        .quads
        .iter()
        .filter(|quad| quad.color == palette.preview_selection)
        .map(|quad| quad.rect)
        .collect();
    for index in [0, 2] {
        assert!(
            bands.iter().any(|band| band[1] == top_of(index)),
            "block {index} is banded on its own row: {bands:?}",
        );
    }
    let carets: Vec<[f32; 4]> = built
        .body
        .quads
        .iter()
        .filter(|quad| quad.color == palette.preview_caret)
        .map(|quad| quad.rect)
        .collect();
    assert_eq!(carets.len(), 1, "one caret on the page: {carets:?}");
    assert_eq!(carets[0][1], top_of(2), "standing in the third block");
}

/// RED (owner's ruling 2026-09-23, B) — **the window draws the span one
/// function computes, holds it while a gesture is in flight, and lets it go
/// when the window loses the pointer.**
///
/// The pure halves are held by their own tests — [`preview_live::source_span`]
/// and [`preview_live::selection_span`] for which blocks, and
/// [`preview_press::held_span`] / [`preview_press::keeps_the_span`] for when,
/// through `preview_press`'s gesture model. What is left is the wiring those
/// cannot see: both arms of `rebuild_preview_document` ask the same producer
/// with the page's rendered selection; the no-parse arm asks it through the
/// hold, keyed on a drag on *this* surface; the press and the drag ask the span
/// on the glass (`doc_key.source`); and a blur ends the drag, which would
/// otherwise hold the span for ever.
///
/// MUTATION: read the fresh span in `rebuild_preview_document` without
/// `held_span` (the page changes shape mid-drag), or drop the blur's
/// `cancel_preview_text_drag` (a lost release pins the span), and a clause
/// below goes red.
#[test]
fn the_window_holds_the_span_a_gesture_starts_on_and_frees_it_on_a_blur() {
    let rebuild = squeezed_body("Runtime", "rebuild_preview_document");
    assert!(
        rebuild.contains("preview_press::held_span(in_flight,")
            && rebuild.contains(".is_some_and(|drag|drag.surface==surface)")
            && rebuild.contains("||self.standing_source_span(surface,Some(caret))"),
        "the span is held while a drag on this surface is in flight",
    );
    assert!(
        rebuild.contains("parsed_source=live_caret.and_then(|caret|{preview_live::selection_span("),
        "a new parse asks the same producer",
    );
    let standing = squeezed_body("Runtime", "standing_source_span");
    assert!(
        standing.contains("preview_live::selection_span(")
            && standing.contains("pane.md_select.as_ref()"),
        "with the page's rendered selection, mapped back to the file",
    );
    let keeps = squeezed_body("Runtime", "preview_press_keeps_the_span");
    assert!(
        keeps.contains("preview_press::keeps_the_span(")
            && keeps.contains("pane.doc_key.as_ref().and_then(|key|key.source.as_ref())"),
        "the press asks about the span on the glass",
    );
    let dispatch =
        item_body(&ItemQuery::method("FolioApp", "window_event").of_trait("ApplicationHandler"));
    let blur = dispatch
        .find("WindowEvent::Focused(false) => {")
        .expect("the window answers losing focus");
    let end = dispatch[blur..]
        .find("WindowEvent::Focused(true) => {")
        .expect("and getting it back");
    assert!(
        dispatch[blur..blur + end].contains("runtime.cancel_preview_text_drag()"),
        "a blur ends the drag, and the span it was holding with it",
    );
}

/// RED (preview report 2026-09-23, A) — **the source block's band is chosen by
/// one function and drawn from it in both faces.**
///
/// The pure half ([`preview_live::source_band`]) is held by its own tests and
/// the monospace painter by
/// `a_source_block_bands_its_rows_from_the_band_it_is_handed`; what is left is
/// the wiring those cannot see, because it lives where the shaper does. The
/// caret's paint asks `source_band` with the page's rendered selection and the
/// in-flight drag's `reached`; the prose face bands `caret.band`; and the drag
/// records `reached` for every gesture — a drag begun on a link spends no press
/// and must still move the source block's band.
///
/// MUTATION: put `.bands(&caret.selection)` back in `build_preview_body_in`, or
/// gate the `reached` write on `spends` again, and a clause below goes red.
#[test]
fn the_source_blocks_band_is_wired_through_one_function_in_both_faces() {
    let paint = squeezed_body("Runtime", "preview_markdown_caret");
    assert!(
        paint.contains("band:preview_live::source_band(caret.range(),pane.md_select.as_ref(),")
            && paint.contains(".and_then(|drag|drag.reached)"),
        "the caret's paint asks source_band with md_select and the drag's reach",
    );
    let body = squeezed_body("Runtime", "build_preview_body_in");
    assert!(
        body.contains(".bands(&caret.band)") && !body.contains(".bands(&caret.selection)"),
        "the prose face bands what it is handed",
    );
    let drag = squeezed_body("Runtime", "drag_preview_text");
    assert!(
        drag.contains("ifletSome(offset)=self.preview_md_file_offset_at(surface,position){")
            && drag.contains("drag.reached=Some(offset);"),
        "every gesture records the byte it reached, not only one the release spends",
    );
    assert!(
        drag.contains("ifreach_moved{self.repaint_preview()?;}"),
        "and a hand moving only inside the source block still asks for a frame",
    );
}

/// RED (preview report 2026-09-23, C1) — **the arm that re-flows without
/// parsing writes a `reflow` line to `BT_PREVIEW_TRACE`.**
///
/// A caret crossing into another block changes only the key's `source`, so
/// `rebuild_preview_document` takes its no-parse arm — and that arm wrote no
/// line, which is why the report's table flip could not be measured. The line
/// is written on that arm, before it returns, with the cause read off the key
/// being replaced and the cost the geometry pass reports. The formatter is held
/// by `preview_trace`'s own test; the count by
/// `a_realize_pass_reports_the_blocks_it_measured`.
///
/// MUTATION: delete the `preview_trace::reflow(` call and the arm is dark
/// again.
#[test]
fn the_no_parse_arm_writes_a_reflow_line() {
    let body = squeezed_body("Runtime", "rebuild_preview_document");
    let arm = body
        .find("ifreflow_only&&")
        .expect("the no-parse arm is where it was");
    let returns = arm
        + body[arm..]
            .find("return;")
            .expect("and it returns before the parse");
    let line = body[arm..returns]
        .find("preview_trace::reflow(trace,")
        .map(|at| arm + at);
    assert!(
        line.is_some(),
        "the no-parse arm writes its line before it returns",
    );
    assert!(
        body[arm..returns].contains("realized:cost.realized")
            && body[..arm].contains("source:new.source!=old.source"),
        "with the blocks it measured and whether the source block moved",
    );
}

/// **The caret's block is drawn as the file's own bytes, and a fence keeps
/// its highlighting while it is** (§7.1.3q, ticket T4).
///
/// The rule on the glass rather than in the layout: the rows the page draws
/// for the source block are the file's lines, in the source face's own line
/// height, with the caret standing where its row and column say — and the
/// fence's syntect walk, which was computed against the fence's *content*,
/// still applies one line down, because the source block draws the markers
/// the content does not have.
///
/// MUTATION ①: draw the block's rendered arm as well as its source rows and
/// the paragraph count doubles — the words are set twice, over themselves.
/// MUTATION ②: read the highlighting at `line` instead of at `line - 1` and
/// the fence's first code line is set in the ink of its own opening
/// backticks, one row out for the whole fence.
#[test]
fn the_carets_block_is_drawn_as_source_and_a_fence_keeps_its_highlighting() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 400.0, 400.0];
    let (left, _) = preview::markdown_measure_box(body, metrics);
    let blocks = prose(&["first", "middle", "last"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let calls = std::cell::Cell::new(0usize);
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        calls.set(calls.get() + 1);
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let source = mono_caret_block(1, 6, "one\ntwo");
    let mono = source
        .mono()
        .expect("a paragraph of source wears the mono face");
    let layout = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        400.0,
        metrics,
        art,
        &mut shaper,
    );
    let caret = MarkdownCaretPaint {
        seat: MarkdownCaretSeat::Source {
            block: source.index(),
            line: 1,
            column: 2,
        },
        lit: true,
        selection: 0..0,
        band: 0..0,
        caret_width: 2.0,
        preedit: None,
    };
    let built = build_preview_markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &intrinsic,
            layout: &layout,
            live: MarkdownLive {
                source: &SourceBlocks::from(source.clone()),
                caret: Some(&caret),
            },
        },
        &palette,
        art,
    );
    let words: Vec<String> = built
        .body
        .paragraphs
        .iter()
        .map(|paragraph| {
            paragraph
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
        })
        .collect();
    assert_eq!(
        words,
        ["first", "one", "two", "last"],
        "the middle block is its own two source lines and is set once",
    );
    let rows: Vec<[f32; 4]> = built.body.paragraphs[1..3]
        .iter()
        .map(|paragraph| paragraph.rect)
        .collect();
    let top = metrics.padding_y + layout[1].top;
    assert_eq!(
        rows[0][1], top,
        "the first row starts at the block's own top"
    );
    assert_eq!(
        rows[1][1] - rows[0][1],
        mono.line_height,
        "and the rows are the source face's line apart",
    );
    assert!(
        built.body.paragraphs[1..3].iter().all(|paragraph| paragraph
            .runs
            .iter()
            .all(|run| run.mono)
            && !paragraph.wrap),
        "monospace, and already folded: a shaper asked to wrap them again \
             would put the second half of a row under the first",
    );
    let [caret_quad] = built.body.quads.as_slice() else {
        panic!("one caret and nothing else: {:#?}", built.body.quads);
    };
    assert_eq!(
        [caret_quad.rect[0], caret_quad.rect[1]],
        [left + mono.advance * 2.0, top + mono.line_height],
        "two columns into the second row",
    );

    // **A fence under the caret keeps its own colours**, offset by the
    // opening line the grammar was never walked over.
    let fence = "```rust\nfn main() {}\n```";
    let blocks = vec![preview::MarkdownBlock::Code {
        lang: Some("rust".to_owned()),
        text: "fn main() {}\n".to_owned(),
    }];
    let intrinsic = vec![MarkdownBlockIntrinsic {
        highlight: markdown_fence_highlight(Some("rust"), "fn main() {}\n"),
        ..MarkdownBlockIntrinsic::default()
    }];
    assert!(
        !intrinsic[0].highlight.is_plain(),
        "the fixture is highlighted, or this proves nothing",
    );
    let source = mono_caret_block(0, 0, fence);
    let layout = lay_markdown_out(
        &blocks,
        &intrinsic,
        &SourceBlocks::from(source.clone()),
        400.0,
        metrics,
        art,
        &mut shaper,
    );
    let built = build_preview_markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &intrinsic,
            layout: &layout,
            live: MarkdownLive {
                source: &SourceBlocks::from(source.clone()),
                caret: None,
            },
        },
        &palette,
        art,
    );
    let runs: Vec<usize> = built
        .body
        .paragraphs
        .iter()
        .map(|paragraph| paragraph.runs.len())
        .collect();
    assert_eq!(runs.len(), 3, "three source lines, markers included");
    assert_eq!(runs[0], 1, "the opening fence is markup and is set plain");
    assert!(
        runs[1] > 1,
        "and the code under it wears the grammar's inks: {runs:?}",
    );
}

/// **A caret between two blocks is drawn as one empty source line under the
/// block in front of it** (§7.1.3q, and [`preview_live`]'s module note for
/// why that slot).
///
/// The gap is the one place the rule as written leaves the caret nowhere to
/// be: the blank line that ends a paragraph belongs to no block and never
/// will, because nothing is built out of it (§7.1.3o).
///
/// MUTATION ①: draw the slot at the *following* block's top and the caret
/// jumps a collapsed margin away from the letters it was just beside — the
/// gesture that reaches a gap is leaving the block above it.
/// MUTATION ②: measure the empty line into the layout and every blank line
/// in a document pushes the page down as the caret walks through it.
#[test]
fn a_caret_in_the_gap_between_two_blocks_is_one_empty_source_line() {
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 400.0, 400.0];
    let (left, _) = preview::markdown_measure_box(body, metrics);
    let blocks = prose(&["first", "second"]);
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let art = PageArt {
        math: &DocumentMath::default(),
        pictures: &DocumentPictures::default(),
        theme: bt_render::Theme::Dark,
    };
    let calls = std::cell::Cell::new(0usize);
    let mut shaper = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        calls.set(calls.get() + 1);
        line * (cell_ink(runs) / width.max(1.0)).ceil().max(1.0)
    };
    let layout = lay_markdown_out(
        &blocks,
        &intrinsic,
        &NO_SOURCE_BLOCKS,
        400.0,
        metrics,
        art,
        &mut shaper,
    );
    let page = |caret: &MarkdownCaretPaint| {
        build_preview_markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(&[]),
            MarkdownPage {
                blocks: &blocks,
                intrinsic: &intrinsic,
                layout: &layout,
                live: MarkdownLive {
                    source: &NO_SOURCE_BLOCKS,
                    caret: Some(caret),
                },
            },
            &palette,
            art,
        )
    };
    let between = MarkdownCaretPaint {
        seat: MarkdownCaretSeat::Gap {
            after: Some(0),
            line_height: 20.0,
        },
        lit: true,
        selection: 0..0,
        band: 0..0,
        caret_width: 2.0,
        preedit: None,
    };
    let built = page(&between);
    assert_eq!(
        built.body.paragraphs.len(),
        2,
        "both blocks are still rendered: a gap turns nothing into source",
    );
    let [quad] = built.body.quads.as_slice() else {
        panic!("one caret: {:#?}", built.body.quads);
    };
    assert_eq!(
        [quad.rect[0], quad.rect[1], quad.rect[2], quad.rect[3]],
        [
            left,
            metrics.padding_y + layout[0].top + layout[0].height,
            left + 2.0,
            metrics.padding_y + layout[0].top + layout[0].height + 20.0,
        ],
        "one line tall, at the column a line starts in, directly under the \
             block the caret has just left",
    );
    let laid_again = lay_markdown_out(
        &blocks,
        &intrinsic,
        &NO_SOURCE_BLOCKS,
        400.0,
        metrics,
        art,
        &mut shaper,
    );
    assert_eq!(layout, laid_again, "and the page did not move to make room");

    let ahead = MarkdownCaretPaint {
        seat: MarkdownCaretSeat::Gap {
            after: None,
            line_height: 20.0,
        },
        ..between.clone()
    };
    let built = page(&ahead);
    assert_eq!(
        built.body.quads[0].rect[1], metrics.padding_y,
        "a caret in front of every block stands at the top of the page",
    );
    let dark = MarkdownCaretPaint {
        lit: false,
        ..between
    };
    assert!(
        page(&dark).body.quads.is_empty(),
        "and a page whose surface does not hold the keyboard draws no caret \
             at all, which is the text face's own rule",
    );
}

/// **An edit to one block re-measures that block and nothing else**
/// (§7.1.3q, ticket T4) — the cache that ended `highlight.rs`'s own line,
/// 「an edit bumps the buffer's revision, which is what re-runs it」.
///
/// Every fence in a document used to be re-walked by syntect and every table
/// re-shaped cell by cell on **every keystroke anywhere in the file**,
/// because the whole vector of intrinsics hung off the buffer's revision.
/// Keyed per block content, an edit inside one fence is one miss.
///
/// MUTATION ①: key the intrinsic on the block's *range* instead of its bytes
/// and the last assertion collapses — one inserted character moves every
/// range after it, so every block below the edit is measured again.
/// MUTATION ②: drop the cache and measure unconditionally — the second
/// assertion goes red on the very same document twice.
#[test]
fn an_edit_to_one_block_keeps_every_other_blocks_measurement() {
    let source = include_str!("../../../docs/UI-UX.md");
    let metrics = seats::preview_markdown_metrics(1.0);
    let palette = bt_render::chrome_palette();
    let math = DocumentMath::default();
    let calls = std::cell::Cell::new(0usize);
    let mut measure = |runs: &[bt_render::PreviewRun], _: f32, _: f32| {
        calls.set(calls.get() + 1);
        runs.iter()
            .map(|run| run.text.chars().count())
            .sum::<usize>() as f32
            * 8.0
    };
    let pass = IntrinsicPass {
        metrics,
        math: &math,
        palette: &palette,
        scale_ppm: scale_ppm(1.0),
        math_generation: 0,
    };
    let mut cache = MarkdownIntrinsicCache::default();
    let (blocks, ranges) = preview::parse_markdown_ranged(source);
    let cold = measure_markdown_intrinsics(
        &blocks,
        MarkdownSourceBytes {
            content: source,
            ranges: &ranges,
        },
        pass,
        &mut cache,
        &mut measure,
    );
    let asked = calls.get();
    assert!(
        asked > 100,
        "the fixture's tables and fences really are a measurable share \
             ({asked} shaping calls), or this proves nothing",
    );

    let warm = measure_markdown_intrinsics(
        &blocks,
        MarkdownSourceBytes {
            content: source,
            ranges: &ranges,
        },
        pass,
        &mut cache,
        &mut measure,
    );
    assert_eq!(
        calls.get(),
        asked,
        "the same document twice is measured once",
    );
    assert_eq!(cold, warm, "and answers the same thing both times");

    // One character typed inside the first fence — which moves the byte
    // range of every block after it, and the content of none of them.
    let fence = blocks
        .iter()
        .position(|block| matches!(block, preview::MarkdownBlock::Code { .. }))
        .expect("the fixture has a fence");
    let inside = source[ranges[fence].clone()]
        .find('\n')
        .map(|at| ranges[fence].start + at + 1)
        .expect("a fence has a line under its opening");
    let mut edited = source.to_owned();
    edited.insert(inside, 'x');
    let (typed, typed_ranges) = preview::parse_markdown_ranged(&edited);
    assert_eq!(
        typed.len(),
        blocks.len(),
        "one character is not a new block"
    );
    assert_ne!(
        typed_ranges[typed_ranges.len() - 1],
        ranges[ranges.len() - 1],
        "and it did move every range after it, which is the point",
    );
    let (hits, misses) = (cache.hits, cache.misses);
    let after = measure_markdown_intrinsics(
        &typed,
        MarkdownSourceBytes {
            content: &edited,
            ranges: &typed_ranges,
        },
        pass,
        &mut cache,
        &mut measure,
    );
    assert_eq!(
        cache.misses - misses,
        1,
        "one block changed, so one block was measured",
    );
    assert!(
        cache.hits - hits > 5,
        "and every other table and fence was remembered ({} of them)",
        cache.hits - hits,
    );
    for (index, (before, now)) in cold.iter().zip(&after).enumerate() {
        if index == fence {
            continue;
        }
        assert_eq!(
            before, now,
            "block {index} kept its measurement and its highlighting",
        );
    }
}

/// Mutation: lose intrinsic/wrapped reuse or parse the body twice per edit.
/// Counts survive scheduler load; wall-clock measurements live in the ignored
/// production-path benchmark in preview_typing (T-MD-TYPING-PATH).
#[test]
fn a_one_character_edit_reuses_unmodified_document_work() {
    preview_typing::assert_one_character_edit_reuses_work();
}

/// RED GATE (user report 2026-08-25) — **a page that just changed size says
/// what size it changed to, in the picture's own words and on the band's own
/// clock.**
///
/// `Ctrl`+wheel moved the engine's `ZoomFactor` and reported nothing, which
/// made it the one gesture in this window that changes what a reader is
/// looking at without saying what it changed it to.
///
/// It was red the day it was written against a
/// [`page_foot_flash`] that answered `None` to everything.
#[test]
fn a_page_says_the_zoom_it_moved_to_and_then_goes_back_to_being_a_hover_line() {
    let now = Instant::now();
    let just_now = now - Duration::from_millis(40);
    let stale = now - FOOT_REVEAL_FEEDBACK - Duration::from_millis(1);

    assert_eq!(
        page_foot_flash(Some((1.2, just_now)), None, None, now).as_deref(),
        Some("120%"),
        "the picture's own words, from the picture's own function"
    );
    // The way back to unzoomed is a rung like any other, and a rung nobody
    // confirms is the one a reader cannot tell they have reached.
    assert_eq!(
        page_foot_flash(Some((1.0, just_now)), None, None, now).as_deref(),
        Some("100%")
    );
    // Whole percents: `0.67` is a rung of the ladder, not a number to read.
    assert_eq!(
        page_foot_flash(Some((0.67, just_now)), None, None, now).as_deref(),
        Some("67%")
    );
    // One clock, and it is the band's own. After it, the hover line again.
    assert_eq!(page_foot_flash(Some((1.2, stale)), None, None, now), None);

    // The later gesture owns the strip — both of these answer something that
    // was just done, and the older one answers a question the hand has left.
    assert_eq!(
        page_foot_flash(Some((1.2, just_now)), Some(stale), None, now).as_deref(),
        Some("120%")
    );
    assert_eq!(
        page_foot_flash(Some((1.2, stale)), Some(just_now), None, now).as_deref(),
        Some(preview_opened_label()),
    );
    assert_eq!(page_foot_flash(None, None, None, now), None);
}

/// **The flip is the view's, not the buffer's** (ruling 2026-08-13).
///
/// Two surfaces on one markdown file: turning one over leaves the other
/// showing what it was showing, and the *buffer* — its body, its dirty bit,
/// its revision — stays shared across both, because the ruling moved the
/// view mode and nothing else.
///
/// Real-machine capture is the reason it exists: `md_source` lived on the
/// shared `PreviewBuffer`, so a float torn off to read a rendered page turned
/// into raw markdown the moment a pane behind it was flipped to source.
///
/// Mutation: read the face off the buffer again — give `PreviewBuffer` back
/// its `md_source` and have both panes consult it — and the second assertion
/// goes red, which is the reported bleed exactly.
#[test]
fn two_surfaces_on_one_markdown_buffer_flip_independently() {
    let mut panes = PreviewPanes::default();
    let source = preview::PreviewSource::file(r"C:\w\notes.md");
    let pane = seat_of(TAB_ONE, SeatId(1));
    let float = PreviewSurface::Float(7);
    for surface in [pane, float] {
        panes.entry(surface).buffer = Some(source.clone());
    }
    let mut pool = preview::PreviewPool::default();
    pool.insert(text_buffer("notes.md", "# Title\n\nbody\n"));

    // Both start on the render, which is what never having been flipped is.
    assert!(!panes.entry(pane).md_source);
    assert!(!panes.entry(float).md_source);

    // The pane is turned over. The float is not.
    panes.entry(pane).md_source = true;
    assert!(panes.entry(pane).md_source, "the surface that was flipped");
    assert!(
        !panes.entry(float).md_source,
        "and only that one — a float reading the page keeps reading the page"
    );

    // Which is what every reader downstream of the flag now answers with.
    let buffer = pool.get(&source).expect("the one buffer");
    assert_eq!(
        (
            buffer.view(panes.entry(pane).md_source),
            buffer.view(panes.entry(float).md_source)
        ),
        (preview::PreviewView::Text, preview::PreviewView::Markdown),
        "one file, two faces, at the same instant"
    );
    assert!(
        buffer.is_editable(true) && buffer.is_editable(false),
        "both faces of one Markdown buffer edit (T5, §7.1.3t) — what differs \
             between them is where the caret is drawn, not whether there is one"
    );
    assert_ne!(
        document_key(buffer, true, 400.0, 1.0),
        document_key(buffer, false, 400.0, 1.0),
        "so the two faces cannot share one cached document"
    );

    // And the buffer itself is still one buffer: the ruling moved the view
    // mode, not the file.
    let buffer = pool.get_mut(&source).expect("the one buffer");
    buffer.edit_content(|content| {
        content.push_str("more\n");
        true
    });
    assert!(buffer.dirty, "an edit through either face dirties the file");
    assert_eq!(
        pool.buffers().count(),
        1,
        "and there is still exactly one of it"
    );
}

/// ⑦ A long markdown and a long table scroll, and neither scrolls past its
/// own end.
///
/// **Slice 2's open account.** Both bodies clamped to `[0, 0]` — the extent
/// function answered `(0.0, 0)` for them — so a `README.md` longer than its
/// pane and a `.csv` with a hundred rows were *drawn* past their own ends
/// and could never be scrolled to. Neither scrolls sideways, and that is a
/// ruling rather than an omission: markdown wraps to the pane and a table's
/// columns are as wide as its own cells.
///
/// Mutation: return `[0.0, 0.0]` for either arm of
/// [`preview_document_max_scroll`], which restores the bug exactly.
#[test]
fn a_long_markdown_and_a_long_table_scroll_and_stop_at_their_own_ends() {
    let body = [0.0, 0.0, 400.0, 200.0];
    let scale = 1.0;
    let metrics = seats::preview_markdown_metrics(scale);
    // Forty blocks of one line each, laid out the way the runtime lays them.
    let blocks: Vec<preview::MarkdownBlock> = (0..40)
        .map(|index| {
            preview::MarkdownBlock::Paragraph(vec![preview::Span::plain(&format!("line {index}"))])
        })
        .collect();
    let mut layout: Vec<MarkdownBlockLayout> = Vec::new();
    let mut top = 0.0_f32;
    for _ in &blocks {
        top += metrics.paragraph_gap;
        layout.push(MarkdownBlockLayout {
            top,
            height: metrics.line_height,
            ..MarkdownBlockLayout::default()
        });
        top += metrics.line_height;
    }
    let last = layout.last().unwrap();
    let expected = last.top + last.height + metrics.padding_y * 2.0 - (body[3] - body[1]);
    let markdown = PreviewDocument::Markdown {
        blocks,
        ranges: Vec::new(),
        maps: Vec::new(),
        source: SourceBlocks::default(),
        intrinsic: Vec::new(),
        layout: layout.into(),
        math: DocumentMath::default(),
        pictures: DocumentPictures::default(),
        wrap: Arc::default(),
    };
    let max = preview_document_max_scroll(&markdown, body, scale, 8.0, 0.0, 0);
    assert!(max[1] > 0.0, "a document taller than its pane can scroll");
    assert_eq!(max[1], expected, "and stops exactly at its own last line");
    assert_eq!(max[0], 0.0, "markdown wraps, so there is nowhere sideways");

    let rows: Vec<Vec<String>> = (0..60)
        .map(|index| vec![format!("row {index}"), "value".to_owned()])
        .collect();
    let table = PreviewDocument::Table {
        rows,
        column_cells: vec![8, 5],
    };
    let max = preview_document_max_scroll(&table, body, scale, 8.0, 0.0, 0);
    let geometry = seats::preview_table_geometry(body, &[8, 5], 60, 8.0, scale, [0.0, 0.0]);
    assert_eq!(max, geometry.max_scroll);
    assert!(max[1] > 0.0, "sixty rows do not fit in two hundred pixels");

    // A short one still cannot be scrolled at all, on either axis.
    let short = PreviewDocument::Table {
        rows: vec![vec!["a".to_owned()]],
        column_cells: vec![2],
    };
    assert_eq!(
        preview_document_max_scroll(&short, body, scale, 8.0, 0.0, 0),
        [0.0, 0.0]
    );
}

/// **A patch wider than its pane wears a rule along its bottom, and the
/// rule can be taken** (user report, 2026-08-17: long `+# …` lines cut at
/// the pane's right edge with no scroller).
///
/// The extent was never the missing half — a diff's `max_scroll[0]` has been
/// right since the ruling above, and `Shift`+wheel already reached it. What
/// was missing was the *picture*: `preview_body_bar` was written with its
/// axis nailed to `Vertical`, so the one body kind in this window with a
/// horizontal extent had no track, no thumb and nothing to put a hand on.
/// The bar is one function and one geometry for both axes
/// (`preview::scroll_bar`), so what the patch grew is the bar the card and
/// the pane already wear, stood on its side — same thickness, same grab
/// tolerance, same linear map from thumb to offset.
///
/// Four claims: the extent is there, the bar is there, the thumb reads back
/// to an offset at both ends of its track, and the offset actually moves the
/// ink.
///
/// MUTATION: put `ScrollAxis::Vertical` back into `preview_body_bar` in
/// place of the parameter. The `across` binding goes `None` and the patch is
/// silently uncrossable again.
#[test]
fn a_patch_wider_than_its_pane_grows_a_rule_a_hand_can_take() {
    let body = [0.0, 0.0, 240.0, 60.0];
    let scale = 1.0;
    let advance = 8.0;
    let metrics = seats::preview_diff_metrics(scale);
    let long = format!("+{}", "# a long comment line".repeat(12));
    let columns = long.chars().count();
    let rows = vec![DiffRow {
        text: long.clone(),
        kind: preview::DiffLineKind::Add,
        top: 0.0,
    }];
    let document = PreviewDocument::Diff(rows.clone());

    // ① The extent, which is what the whole bar is a picture of.
    let max = preview_document_max_scroll(
        &document,
        body,
        scale,
        advance,
        metrics.line_height,
        columns,
    );
    assert!(
        max[0] > 0.0,
        "a patch does not reflow, so it keeps the horizontal scroll it needs"
    );

    // ② The bar. `page + max_scroll` is the content width by construction —
    //    the same inverse `preview_surface_bar` takes.
    let content = (body[2] - body[0]) + max[0];
    let across = preview_body_bar(
        body,
        preview::ScrollAxis::Horizontal,
        [0.0, 0.0],
        content,
        scale,
    )
    .expect("a patch wider than its pane has a rule along the bottom");
    assert_eq!(across.axis, preview::ScrollAxis::Horizontal);
    assert!(
        across.track[3] <= body[3] && across.track[1] >= body[1],
        "the rule lies against the body's own bottom edge, {:?}",
        across.track
    );
    assert!(
        (across.overflow - max[0]).abs() < 0.01,
        "and it promises exactly as far as the clamp allows: {} against {}",
        across.overflow,
        max[0]
    );

    // ③ The thumb reads backwards to an offset, at both ends of its track.
    assert_eq!(
        preview::scroll_dragged_to(&across, across.track_start(), 0.0),
        0.0,
        "the thumb at the near end is the document at its own left"
    );
    assert!(
        (preview::scroll_dragged_to(&across, across.track[2], 0.0) - across.overflow).abs() < 0.01,
        "and dragged to the far end it lands exactly where the wheel stops"
    );

    // ④ And the offset moves the ink. The band under an added line does
    //    *not* move — it is the viewport's, by `band_rect`'s own ruling —
    //    so this asks the paragraph, which is the text.
    let palette = bt_render::chrome_palette();
    let drawn_at = |scroll: [f32; 2]| {
        let geometry = seats::preview_mono_geometry(
            body,
            metrics,
            metrics.line_height,
            columns,
            advance,
            scroll,
        );
        build_preview_diff_body(&geometry, &rows, &palette).paragraphs[0].rect[0]
    };
    let still = drawn_at([0.0, 0.0]);
    let shifted = drawn_at([max[0], 0.0]);
    assert!(
        (still - shifted - max[0]).abs() < 0.01,
        "scrolling to the far end moves the run left by the whole extent: \
             {still} then {shifted}, extent {}",
        max[0]
    );
}

/// **§7.12 ⓑ — a preview surface is resolved in the tab it names, and the
/// two tabs below name the same seat number.**
///
/// The value half of
/// `tab_identity_tests::a_preview_surface_is_read_and_written_in_the_one_tab_that_owns_it`,
/// on two tabs a real window builds the ordinary way: every tree numbers its
/// own seats from 1, so a window with two tabs each holding one terminal and
/// one preview has **two panes on `SeatId(2)`** — and it always did.
///
/// What the defect cost is the last two assertions. Every window-level door
/// into a preview resolved a seat number by walking the strip for a tab whose
/// tree held it, so the answer was always the *first* such tab: with the
/// second tab in front, a scroll, a keystroke, a save and a goto on the pane
/// you were looking at all landed in the pane you were not.
///
/// MUTATION — **the seat number is the whole name again**: have
/// [`TabState::preview_here`] answer
/// `PreviewSurface::Seat(LeafId { tab: TabId(1), seat })` whatever tab it is
/// asked of. Both panes become one surface, `preview_tab_index_among`
/// answers `Some(0)` for both, and the two reads at the bottom come back
/// `todo.txt` twice and `README.md` never — which is the defect, in the
/// three lines it takes to see it.
#[test]
fn a_preview_seat_is_found_in_the_tab_that_owns_it() {
    let (first, first_seat) = tab_with_a_preview(
        1,
        vec![buffer_saying(r"D:\notes\todo.txt", "todo.txt", "milk\n")],
    );
    let (second, second_seat) = tab_with_a_preview(
        2,
        vec![buffer_saying(r"D:\notes\README.md", "README.md", "# hi\n")],
    );
    assert_eq!(
        first_seat, second_seat,
        "two tabs built the ordinary way number their preview pane alike — \
             this is the state, not a contrivance"
    );

    let tabs = [first, second];
    let here = tabs[0].preview_here(first_seat);
    let next_door = tabs[1].preview_here(second_seat);
    assert_ne!(here, next_door, "and they are still two surfaces");

    let PreviewSurface::Seat(here_leaf) = here else {
        unreachable!("a preview pane is a seat surface");
    };
    let PreviewSurface::Seat(next_door_leaf) = next_door else {
        unreachable!("a preview pane is a seat surface");
    };
    assert_eq!(
        preview_tab_index_among(&tabs, here_leaf.tab),
        Some(0),
        "the first tab's pane is found in the first tab"
    );
    assert_eq!(
        preview_tab_index_among(&tabs, next_door_leaf.tab),
        Some(1),
        "and the second tab's pane in the second — the seat number says \
             nothing about which, and is not consulted"
    );

    // …which is what makes the content right, and the content is the whole
    // report: this is the read a scroll, an edit and a save each make before
    // they write back through the very same lookup.
    let read = |surface: PreviewSurface| {
        let PreviewSurface::Seat(leaf) = surface else {
            unreachable!("a preview pane is a seat surface");
        };
        let index = preview_tab_index_among(&tabs, leaf.tab)?;
        let tab = &tabs[index];
        tab.preview_pool
            .get(tab.preview_panes.get(surface)?.buffer.as_ref()?)
            .map(|buffer| buffer.name.clone())
    };
    assert_eq!(read(here).as_deref(), Some("todo.txt"));
    assert_eq!(read(next_door).as_deref(), Some("README.md"));
}

/// RED — **the wiring the headless tests above cannot stand in front of.**
///
/// Sentences about a `Runtime`, which needs a device layer and a window to
/// exist, held against the source itself the way this file's other
/// structural promises are. Each of them is one half of a mechanism whose
/// other half is pinned by a real test above — the last two by
/// `preview::tests`' own read-landing cases (ticket T-EDIT-DISK).
///
/// RED GATE: delete any one of the lines named and its assertion goes red;
/// on a real machine each is one of the two roads, one of the two moments,
/// or the answer coming back, going quiet.
#[test]
fn the_disk_news_reaches_the_glass_by_both_roads() {
    let watched = method_body("Runtime", "watched_preview_files");
    assert!(
        watched.contains("files_a_tab_stands_on"),
        "the window's set is the fold of the per-tab answer the tests above check"
    );
    let refresh = method_body("Runtime", "refresh_preview_file");
    assert!(
        refresh.contains("note_disk_moved(news.present, news.modified)"),
        "and the watcher's news goes through the one door that knows the three cases apart"
    );
    assert!(
        refresh.contains("self.request_stale_previews(index)"),
        "a pooled buffer no pane is on is asked for through the pool's own door"
    );
    // **And the picture lane, which has no buffer to put the news through**
    // (user report 2026-08-31). What a document does with `note_disk_moved`
    // a picture does by having this window forget what it remembers of it;
    // the byte count beside it is the one field of the meta line no decoder
    // answers, so it is asked again by name.
    assert!(
        refresh.contains("self.forget_the_picture_in(path)"),
        "a picture whose file moved is forgotten, so the next frame asks the disk"
    );
    assert!(
        refresh.contains("want: preview::PreviewWant::Size"),
        "and the meta line's byte count is asked again with it"
    );
    // **And the road back** (ticket T-EDIT-DISK). A re-read is issued while
    // the body is the file's and lands whenever the disk gets round to it,
    // so the answer is reconciled with the body the buffer is holding *now*
    // — the rule itself is pinned on the buffer in `preview.rs`, and this is
    // the one line that puts this window's answers through it.
    let landing = method_body("Runtime", "apply_preview_results");
    assert!(
        landing.contains("land_read(outcome, base)"),
        "a read answers the body it was issued for, and a reader who typed \
             while it was in flight keeps what they typed"
    );
    assert!(
        landing.contains("settle_preview_disk_notices()"),
        "and a refused read's strip goes up the way every other strip does"
    );
    // The other two doors a picture arrives by. Both are gestures, and both
    // are moments this window is about to draw a file it has not looked at.
    for door in ["open_preview_image_on", "request_revived_previews"] {
        assert!(
            method_body("Runtime", door).contains("self.forget_the_picture_in("),
            "`{door}` draws a picture out of memory instead of off the disk"
        );
    }
    let settle = method_body("Runtime", "settle_pane_notices");
    assert!(
        settle.contains("self.seats.preview_seats()"),
        "and a preview seat is one of the seats that can wear the strip"
    );
    // The fallback's two moments (rule 4).
    // The call is split across two lines, which is why the deleted reading
    // trimmed every line of the file and joined them again. It is one call of
    // one name inside one item, and that is how it is asked now.
    assert!(
        !found_in(
            needle!(Pattern::call("ask_the_unwatched_preview_files")),
            View::Identifiers,
            Scope::Item(
                ItemQuery::method("FolioApp", "window_event").of_trait("ApplicationHandler")
            ),
        )
        .is_empty(),
        "a window given focus asks about the files no kernel speaks for"
    );
    let land = method_body("Runtime", "land_preview_source_on");
    assert!(
        land.contains("self.ask_the_unwatched_preview_files()?;"),
        "and so does a document being brought to the front"
    );
}

/// **The third media lane, and the finding is that it needed nothing**
/// (§7.44 ⑮, 2026-08-30 — the `.gif` third of defects #202/#204).
///
/// Three lanes were walked across a tab boundary and two of them were
/// broken. This is the one that was not, and it is written down because
/// "nothing to do here" is a claim that can stop being true: an animation is
/// keyed by the **file** and not by the surface
/// ([`WindowRuntime::animations`], §7.44 ⑤ — which is what puts three
/// surfaces showing one `loading.gif` on the same frame), and the file
/// travels with the pane on [`PreviewImageState::path`]. So a move re-keys
/// nothing, because there is no key with an address in it.
///
/// Both halves are asserted: that the reader is by file, and that the file
/// crosses. Either one alone would go on passing while the pair stopped
/// being a reason.
///
/// RED GATE: key `animations` by `PreviewSurface` and the first assertion
/// goes red — and the lane joins the other two, needing a carrier of its own.
#[test]
fn an_animation_crosses_a_tab_boundary_because_it_is_keyed_by_its_file() {
    let running = squeezed_body("Runtime", "animation_running_on");
    assert!(
        running.contains("normalized_local_image_path_key"),
        "the animation lane is read by file, which is why a move has \
             nothing to re-key:\n{running}"
    );
    let names = squeezed_body("Runtime", "animation_path_of");
    assert!(
        names.contains("self.preview_picture(surface)?.path"),
        "and the file it is read by is the one on the pane, which travels \
             with the pane:\n{names}"
    );

    let (mut origin, gif_seat) = tab_with_a_picture(1, r"D:\shots\folio-anim-test.gif");
    let mut alone = tear_pane_into_tab(
        &mut origin,
        &cross_metrics(),
        gif_seat,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("an animated picture may become a tab of its own");
    let lone_seat = alone.seats.preview_seats()[0];
    assert_eq!(
        pictures_drawn(&alone),
        vec![Path::new(r"D:\shots\folio-anim-test.gif")],
        "the file crossed the tear-out"
    );

    let moved = cross_move(
        &mut alone,
        &mut origin,
        lone_seat,
        seats::DropEdge::Right,
        true,
    )
    .expect("and comes back the way it went");
    assert_eq!(
        origin
            .preview_panes
            .get(seat_of(TabId(1), moved.landed))
            .and_then(|pane| pane.image.as_ref())
            .map(|picture| picture.path.as_path()),
        Some(Path::new(r"D:\shots\folio-anim-test.gif")),
        "and the surface the animation is asked about names the same file, \
             so the clock it is on is the clock it was on"
    );
}

/// PIN — a seat and a float are two surfaces even when their numbers agree.
///
/// `SeatId` and `FloatId` are both counters that start at the low integers, so
/// a content plane keyed on a bare number would have the first preview pane
/// and the first preview float share a view — one scroll, one caret, and a
/// document that jumps when you touch the other window.
///
/// Mutation: give `PreviewSurface` a `PartialEq` that compares only the
/// number inside — `panes.entry(float)` then finds the seat's view and the
/// map holds one entry answering for two windows.
#[test]
fn a_seat_and_a_float_that_share_a_number_are_still_two_surfaces() {
    let seat = seat_of(TAB_ONE, SeatId(1));
    let float = PreviewSurface::Float(1);
    assert_ne!(seat, float);
    let mut panes = PreviewPanes::default();
    panes.entry(seat).scroll = [0.0, 10.0];
    panes.entry(float).scroll = [0.0, 20.0];
    assert_eq!(panes.get(seat).expect("a view").scroll, [0.0, 10.0]);
    assert_eq!(panes.get(float).expect("a view").scroll, [0.0, 20.0]);
}

/// **A fence is highlighted by its info string, and only by it** (#49).
///
/// Both halves in one test because they are one ruling: ` ```rust ` gets the
/// palette's keyword and number inks, and a fence with no info string keeps
/// the single `--ink2` run it has always been drawn as. The second half is
/// the one that would rot silently — a highlighter that sniffed the contents
/// would pass every assertion about the first fence and quietly colour a
/// directory listing as though it were code.
///
/// MUTATION: drop the `syntax_for_fence` guard in `markdown_fence_highlight`
/// and fall back to guessing, and the second half goes red.
#[test]
fn a_fence_is_highlighted_by_its_info_string_and_by_nothing_else() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 600.0, 600.0];
    let source = "let count = 42; // how many\n";
    let blocks = [
        preview::MarkdownBlock::Code {
            lang: Some("rust".to_owned()),
            text: source.to_owned(),
        },
        preview::MarkdownBlock::Code {
            lang: None,
            text: source.to_owned(),
        },
    ];
    let intrinsic: Vec<MarkdownBlockIntrinsic> = blocks
        .iter()
        .map(|block| match block {
            preview::MarkdownBlock::Code { lang, text } => MarkdownBlockIntrinsic {
                rows: 1,
                highlight: markdown_fence_highlight(lang.as_deref(), text),
                ..MarkdownBlockIntrinsic::default()
            },
            _ => MarkdownBlockIntrinsic::default(),
        })
        .collect();
    let fence_height = metrics.line_height + (metrics.code_border + metrics.code_padding_y) * 2.0;
    let layout: preview_viewport::Layout = [
        MarkdownBlockLayout::solid(fence_height),
        MarkdownBlockLayout {
            top: fence_height + metrics.code_margin,
            ..MarkdownBlockLayout::solid(fence_height)
        },
    ]
    .into();
    let rendered = build_preview_markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        MarkdownPage {
            blocks: &blocks,
            intrinsic: &intrinsic,
            layout: &layout,
            live: MarkdownLive::default(),
        },
        &palette,
        PageArt {
            math: &DocumentMath::default(),
            pictures: &DocumentPictures::default(),
            theme: bt_render::Theme::Dark,
        },
    );
    // The two fences' lines, in the order they were pushed. The `lang` chip
    // is a proportional paragraph and rides after them, so the two mono
    // paragraphs are the first two.
    let mono: Vec<&bt_render::PreviewParagraph> = rendered
        .body
        .paragraphs
        .iter()
        .filter(|paragraph| paragraph.runs.iter().all(|run| run.mono))
        .collect();
    assert_eq!(mono.len(), 2, "one line drawn per fence");

    let told = &mono[0];
    assert!(
        told.runs.len() > 1,
        "the rust fence is cut into runs: {:?}",
        told.runs.iter().map(|run| &run.text).collect::<Vec<_>>()
    );
    assert!(
        told.runs.iter().any(|run| run.color == palette.hl_keyword),
        "`let` wears the keyword ink"
    );
    assert!(
        told.runs.iter().any(|run| run.color == palette.hl_number),
        "`42` wears the number ink"
    );
    assert!(
        told.runs.iter().any(|run| run.color == palette.hl_comment),
        "the trailing comment wears the comment ink"
    );
    assert!(
        told.runs
            .iter()
            .any(|run| run.color == palette.preview_code_text),
        "and `count` is left the fence's own ink — the body ink inside a \
             fence is `--ink2`, not the pane's `--ink`"
    );
    assert_eq!(
        told.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        source.trim_end_matches('\n'),
        "the runs put the line back together exactly"
    );

    let untold = &mono[1];
    assert_eq!(untold.runs.len(), 1, "an unlabelled fence is one run");
    assert_eq!(untold.runs[0].color, palette.preview_code_text);
    assert_eq!(untold.runs[0].text, source.trim_end_matches('\n'));
}

/// PIN (W2 slice 5) - **which content lane a row activated in the files
/// column opens on.**
///
/// The fork, asked of six kinds of name and one policy. The three claims the
/// ticket asks for are all here:
///
/// * `.html`, `.htm` and `.pdf` open a **page** - and the PDF is one line of
///   this table and nothing else, which is `plan.md` section 0's
///   "PDF: 类型路由一行" made literal;
/// * a page on a **network share** is not a page: `webnav::Mint::file`
///   refuses to mint one, so it goes down the document lane whose
///   `NetworkPath` card is the refusal this window has always shown;
/// * **everything else is untouched.** A picture still goes to the decoder
///   and every other name still goes to the pool, which is the regression
///   half: this slice added a lane, it did not move one.
///
/// RED GATE: drop the `pdf` arm of [`path_opens_as_a_page`] - the first
/// group fails on `report.pdf` alone, which is exactly how much of this
/// feature PDF is.
#[test]
fn a_row_opens_on_the_lane_its_name_and_the_mint_agree_on() {
    for page in [
        r"D:\site\index.html",
        r"D:\site\index.htm",
        r"D:\site\INDEX.HTM",
        r"D:\reports\report.pdf",
        r"D:\reports\REPORT.PDF",
    ] {
        assert_eq!(
            preview_open_lane(Path::new(page)),
            PreviewOpenLane::Page,
            "{page}"
        );
    }
    for share in [
        r"\\server\share\index.html",
        r"\\server\share\report.pdf",
        r"\\?\UNC\server\share\index.htm",
    ] {
        assert_eq!(
            preview_open_lane(Path::new(share)),
            PreviewOpenLane::Document,
            "a share is refused at the mint, and the document lane has the \
                 card that says so: {share}"
        );
    }
    for picture in [r"D:\shots\a.png", r"D:\shots\a.SVG"] {
        assert_eq!(
            preview_open_lane(Path::new(picture)),
            PreviewOpenLane::Picture,
            "{picture}"
        );
    }
    for document in [
        r"D:\src\main.rs",
        r"D:\README.md",
        r"D:\cases.csv",
        r"D:\a.exe",
        // The two neighbours a substring reading would sweep up.
        r"D:\site\index.htmlx",
        r"D:\site\report.html.txt",
        r"D:\site\notes.pdfx",
    ] {
        assert_eq!(
            preview_open_lane(Path::new(document)),
            PreviewOpenLane::Document,
            "{document}"
        );
    }
}

/// PIN — **no video takes the page lane, at any door** (measured
/// 2026-08-25; `docs/DESIGN.md` §7.16, still standing under §7.23).
///
/// The negative half of the same measurement `preview`'s own pin carries:
/// WebView2 has no viewer for a top-level media response, so it turns one
/// into a download and the platform bridge cancels every download — the
/// navigation completes `ConnectionAborted` and the seat draws 「did not
/// respond」. Put video on the page lane and that browser error replaces
/// whatever honest thing the seat was showing.
///
/// **It is asserted at every door and not only at the double click**,
/// because that is the shape §7.10 ⑥ exists to protect: the head's `↗` and
/// the pool's own door read the same predicate, so a video must be absent
/// from all three or present in all three. The day something hosts a video
/// inside a page, these are the lines that say what changes.
///
/// **What 2026-08-27 changed, and what it did not.** It did not change this:
/// a video is still not a page and the three page doors still answer
/// nothing. What it changed is what the *other* lane is — a video now has a
/// class and a lane of its own ([`a_video_has_a_face_of_its_own`]) instead
/// of falling through to the pool's refusal — so this test asserts the
/// exclusion rather than the destination, which is the claim that was always
/// being made.
///
/// The list is deliberately wider than `preview::VIDEO_EXTENSIONS`: `.mov`
/// and `.mkv` are not in that table, they are ordinary documents, and they
/// are here because "not a page" is a claim about every video spelling and
/// not only about the three this window drew a face for.
///
/// **What route B changed, and what it did not** (2026-08-28; §7.44 ④).
/// The claim is the same claim and it is now simpler than it has ever been:
/// **no door opens a video as a page, and there is no longer a door that
/// puts a video near one.** Route A's play verb wrote a shell page and
/// navigated a browser at it; that verb, that page and that mint are gone,
/// and a recording is decoded by Media Foundation and drawn on this window's
/// own glass. §7.16's measurement stands exactly as measured — a top-level
/// navigation to a `.mp4` is still a download, still cancelled, still
/// `ConnectionAborted` — and this slice did not test it again because it did
/// not change it. What it did was remove the only reason anyone would.
///
/// RED GATE: add `mp4` to `preview::PAGE_EXTENSIONS` and every assertion in
/// the loop fails, each naming the door it stands at.
#[test]
fn a_video_takes_no_page_lane_at_any_door() {
    for video in [
        r"D:\shots\clip.mp4",
        r"D:\shots\CLIP.MP4",
        r"D:\shots\trailer.m4v",
        r"D:\shots\screencast.webm",
        r"D:\shots\clip.mov",
        r"D:\shots\clip.mkv",
    ] {
        assert_ne!(
            preview_open_lane(Path::new(video)),
            PreviewOpenLane::Page,
            "the double click: {video}"
        );
        assert_eq!(
            preview_page_hand_off(&preview::PreviewSource::file(video)),
            None,
            "the head's ↗: {video}"
        );
        assert_eq!(
            source_opens_as_a_page(&preview::PreviewSource::file(video)),
            None,
            "the pool's own door: {video}"
        );
    }
}

/// RED — **a video has a face of its own, and every door draws it** (user
/// ruling 2026-08-27; §7.23).
///
/// The positive half, and the thing the slice is: until it, all six
/// spellings above answered `Document` at the door and `Refused` on the
/// card, which on the machine was "No preview for this file type" under a
/// pointer and the same sentence larger under a double click. Three of them
/// now answer with a lane and a body that show the file.
///
/// **Both halves have to be here.** The first group is the promise: the
/// three spellings in `preview::VIDEO_EXTENSIONS` take the video lane, are
/// classed `video`, and earn the frame body on the card. The second is the
/// bound on it: `.mov`, `.mkv` and a name that merely *contains* a video
/// extension are untouched, still documents, still refused — because a class
/// that quietly widened would be a face promised over files this window
/// cannot open.
///
/// RED GATE ①: revert `preview_open_lane`'s video arm and the lane assertion
/// fails for all three, which is the double click going back to the refusal
/// card. RED GATE ②: send `PreviewFtype::Video` to `PeekBodyKind::Refused`
/// and the card assertion fails while the lane one stays green — the pane
/// showing a frame while the card over the same row says there is no
/// preview, which is exactly the contradiction §7.10 ⑥ exists to forbid.
#[test]
fn a_video_has_a_face_of_its_own() {
    let ftype_of = |path: &str| {
        preview::preview_ftype(
            Path::new(path)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .expect("a name"),
        )
    };
    for video in [
        r"D:\shots\clip.mp4",
        r"D:\shots\CLIP.MP4",
        r"D:\shots\trailer.m4v",
        r"D:\shots\screencast.webm",
        // **And the four that route A could not play** (route B slice ②,
        // 2026-08-28; §7.44 ⑥). The first cut of this list kept them out on
        // the argument that "has a face" and "can be played" would one day
        // be one set. They are one set now, and it was one decoder that made
        // them one: every name here was handed to `Engine::open` on the
        // machine and gave up frames. See
        // `preview::VIDEO_EXTENSIONS`, and the class's own gate
        // `every_name_in_the_class_plays_and_the_class_is_the_seven_that_were_opened`.
        r"D:\shots\capture.mov",
        r"D:\shots\clip.mkv",
        r"D:\shots\clip.avi",
        r"D:\shots\recording.wmv",
    ] {
        assert_eq!(
            preview_open_lane(Path::new(video)),
            PreviewOpenLane::Video,
            "the double click opens the frame: {video}"
        );
        assert_eq!(
            ftype_of(video),
            preview::PreviewFtype::Video,
            "and the chip says so: {video}"
        );
        assert_eq!(
            peek_body_kind(ftype_of(video), Some(Path::new(video)), false, false),
            PeekBodyKind::Frame,
            "and the glance card shows the same thing the door opens: {video}"
        );
    }
    for document in [
        // The two containers that are still outside the class, and outside
        // it for the honest reason: nobody has opened one on the machine, so
        // there is no fixture and no measurement behind a row (§7.44 ⑪ ⓒ).
        r"D:\shots\clip.mpg",
        r"D:\shots\clip.flv",
        // And the two neighbours a substring reading would sweep up.
        r"D:\shots\clip.mp4.txt",
        r"D:\shots\clip.webmx",
    ] {
        assert_eq!(
            preview_open_lane(Path::new(document)),
            PreviewOpenLane::Document,
            "outside the class is untouched: {document}"
        );
    }
    // A composed document that merely spells a video's name has nothing to
    // decode: a git diff of `clip.mp4` is a reading of a repository.
    assert_eq!(
        peek_body_kind(preview::PreviewFtype::Video, None, false, false),
        PeekBodyKind::Refused,
        "a body with no file behind it is not a frame"
    );
}

// **`only_a_playable_video_is_offered_a_play_button` is retired here**
// (route B slice ②, 2026-08-28; §7.44 ⑥), and the retirement is the ruling
// rather than a tidy-up.
//
// It asserted the *second column* of the class table — a name with a face
// and no player — and that column existed for exactly one reason: the still
// came from Media Foundation and the playback came from Chromium, so two
// decoders could disagree about one file. One decoder answers both
// questions now, the column has no member, and a test that asserted
// `.mov` is not playable would today be asserting the defect.
//
// What replaces it is `preview`'s own
// `every_name_in_the_class_plays_and_the_class_is_the_seven_that_were_opened`,
// which is stronger than what stood here: it names all seven, it asserts
// that the face and the play button read **one** predicate so they cannot
// come apart by construction, and every row behind it was opened on the
// machine rather than declared.

/// RED — **a playing video is a file to every surface that names it**
/// (user ruling 2026-08-27; re-based on route B, 2026-08-28; §7.23 ⑩, §7.44
/// ①).
///
/// Route A's whole cost was identity, and this is where it was paid: what
/// the engine was on was a page in a cache folder called `play-3f2c….html`,
/// and what the reader opened was `D:\shots\clip.mp4`. Route B does not pay
/// it — there is no second file — but the readings this test pins were never
/// about the shell, they were about a pane whose picture has been replaced
/// by something moving, and every one of them is still owed.
///
/// Read out of the source because there is no value that expresses "the rail
/// asks what this surface is playing": a rail's kind is computed from a
/// window, and standing one up in a unit test is standing up a window.
///
/// RED GATE ①: drop the `!self.surface_is_playing_a_video(surface)` from
/// `preview_rail_kind` and the first assertion fails — a playing pane grows
/// an address bar where its breadcrumb was. RED GATE ②: drop it from
/// `refit_preview_picture` and the still is painted straight over the moving
/// picture, which on the machine is a video that plays for one frame and
/// then freezes.
#[test]
fn a_playing_video_is_spelled_as_the_file_it_is() {
    let playing = concat!("surface_is_playing", "_a_video(surface)");
    let body = |name: &str| method_body("Runtime", name);
    assert!(
        body("preview_rail_kind").contains(playing),
        "a playing pane must wear its file's breadcrumb and not an address bar"
    );
    assert!(
        body("refit_preview_picture").contains(playing),
        "and the decoded still must come off the glass while the engine is drawing"
    );
    assert!(
        body("preview_head_tools").contains(playing),
        "and the head's per-type slot must hold the stop rather than a flip"
    );
    // **And the one place a surface's recording is known** — one map, keyed
    // by surface. Route A had to ask a browser, which asked its mint; there
    // is no browser and no mint, so there is nowhere for a second answer to
    // come from.
    assert!(
        body("video_playing_on").contains("self.window.video.get(surface)"),
        "the window asks its own map and nothing else"
    );
}

/// RED — **the tick that empties the picture list still hands it over**
/// (§7.44 ⑨, photographed on the machine 2026-08-28).
///
/// The freeze above and this are the same mistake twice, one line apart: a
/// gate that decides there is nothing to say by asking about the thing that
/// has just stopped existing.
///
/// `advance_strip_animation` skips `refresh_video_layers` while nothing is
/// moving, and "nothing is moving" was read as *this window holds no
/// recording and no animation*. But `sweep_video_seats` runs on the line
/// above, and removing the last seat is exactly what makes that true — so
/// the one tick where the renderer needed to be told the list is now empty
/// is the one tick that never told it. It went on drawing the last frame it
/// was given.
///
/// **What that looks like:** a floating window playing a recording is
/// closed, and its last decoded frame stays on the glass with no head, no
/// bar and no window around it, until something else happens to run a layout
/// pass. The decoder is already gone by then, which is why the leftover is a
/// photograph rather than a video — four captures 700ms apart, byte
/// identical over that rectangle.
///
/// The mend keeps the gate's real purpose — an idle window rebuilds nothing
/// — by asking the renderer as well: *and the renderer is holding nothing*.
///
/// A source pin because the gate is a `let` inside a method that cannot be
/// called without a window, and because what has to be true is about the
/// *condition* rather than about a value: a build where the third clause is
/// missing is green on every machine that never closes a video.
///
/// RED GATE: drop the `renderer.video_layers()` clause and this fails, which
/// is the state the binary photographed on 2026-08-28 was built from.
#[test]
fn the_tick_that_empties_the_picture_list_still_hands_it_over() {
    // Read off the *service* since the closure review of 2026-09-18 moved
    // these lines out of the tick and above the display gate; the property
    // is unchanged and so is the order it is about.
    let tick = method_body("Runtime", "service_pictures");
    let guard = tick
        .find("let anything_moving =")
        .expect("the service still gates the picture list");
    let end = tick[guard..].find(';').expect("the gate is one statement") + guard;
    let condition = &tick[guard..end];
    assert!(
        condition.contains("self.window.renderer.video_layers().is_empty()"),
        "the gate decides there is nothing to hand over without asking the \
             renderer what it is still holding, so the tick that removes the last \
             seat never tells it:\n{condition}"
    );
    // And the sweep is above it, which is what makes the gap reachable at
    // all — a sweep that ran afterwards would leave the stale list for one
    // tick and no longer.
    let sweep = tick
        .find("self.sweep_video_seats();")
        .expect("the service sweeps the seats");
    assert!(
        sweep < guard,
        "the seats are swept after the list is gated, which is a different \
             defect from the one this pins"
    );
}

/// RED — **a surface handed something else stops playing what it was
/// playing** (user report on `next16`, 2026-08-28; `docs/DESIGN.md` §7.44
/// ⑬).
///
/// The reader played a recording in a preview pane, then clicked a `.md` in
/// the files column. The markdown rendered — and a violet rectangle stood
/// across the middle of it with the player's control bar still under it,
/// counting. The violet was `folio-video-test.wmv`'s own colour: nothing was
/// broken about the *drawing*, the seat was simply never retired, so a
/// decoder went on handing frames to a layer that went on being drawn.
///
/// `sweep_video_seats` is the one door that retires a seat and it asked the
/// right question of the wrong lane: "what picture is this surface showing".
/// A markdown file is not a picture, so the answer was `None` — which the
/// sweep reads as *this surface has not said yet* and keeps the seat for,
/// because that is exactly what a float looks like for one call in the
/// middle of a tear-off. One reading, two situations, and the grace written
/// for the second swallowed the first.
///
/// Three assertions, and the middle one is the mend:
///
/// ① **Every way a surface's content can change ends the recording.** The
/// three the reader can reach — another document, another picture, another
/// recording — plus the page lane, put to [`surface_subject_of`] over a real
/// [`PreviewPane`] mutated exactly the way the landing doors mutate it.
///
/// ② **And the tear-off's grace survives.** A surface with nothing filed on
/// it keeps its seat, which is the one case the old reading was right about
/// and the case a blunter mend would have broken.
///
/// ③ **The engine goes with the seat.** §7.42 ⑦'s process-wide counter comes
/// back to where it started, so "the decoder stopped" is arithmetic rather
/// than a hope.
///
/// And a source pin, because the sweep itself is a closure over a `Runtime`
/// no test here can build: it asks `preview_subject` and no longer asks the
/// picture lane.
///
/// RED GATE: delete the buffer arm from [`surface_subject_of`] — which is
/// the whole of what the sweep consulted before this commit — and the
/// markdown case in ① answers `Unfilled`, the seat lives, and ③'s counter
/// never comes back down.
#[test]
fn a_seat_that_changes_content_drops_its_video() {
    use bt_platform::video::engine::engines_outstanding;
    let _ledger = ledger_gate();
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets");
    // The recording that plays is the one every host's engine decodes — H.264
    // in MP4; AVFoundation has no source for WMV, so a `.wmv` seat opens no
    // engine on a Mac and ③'s counter would have nothing to come back from.
    let recording = assets.join("folio-video-test.mp4");
    let another = assets.join("folio-video-test.wmv");
    let document = assets.join("md-image-check.md");
    let picture = assets.join("folio-anim-test.gif");

    // A pane playing the recording, filled the way `open_preview_image_on`
    // fills it.
    let playing = |path: &std::path::Path| PreviewPane {
        image: Some(PreviewImageState::new(path.to_path_buf())),
        ..PreviewPane::default()
    };
    let pane = playing(&recording);
    assert_eq!(
        surface_subject_of(Some(&pane), false),
        SurfaceSubject::File(recording.clone()),
        "a pane showing a recording is about that recording"
    );
    assert!(surface_subject_of(Some(&pane), false).is_still(&recording));

    // ① the four ways it stops being about it.
    //
    // A document, landed the way `land_preview_source_on` lands one: the
    // picture is cleared and a buffer takes its place.
    let after_a_document = PreviewPane {
        buffer: Some(preview::PreviewSource::file(document.clone())),
        ..PreviewPane::default()
    };
    // Another picture and another recording both come down the picture
    // lane, which is the one case the old reading did catch.
    let after_a_picture = playing(&picture);
    let after_another = playing(&another);
    // A commit graph: content with no path of its own.
    let after_a_graph = PreviewPane {
        buffer: Some(preview::PreviewSource::GitGraph {
            root: assets.clone(),
        }),
        ..PreviewPane::default()
    };
    for (what, pane, page) in [
        ("a markdown document", &after_a_document, false),
        ("another picture", &after_a_picture, false),
        ("another recording", &after_another, false),
        ("a commit graph", &after_a_graph, false),
        // A page is the fourth lane, and it fills none of the fields above.
        ("a live page", &PreviewPane::default(), true),
    ] {
        assert!(
            !surface_subject_of(Some(pane), page).is_still(&recording),
            "a surface showing {what} is still credited with the recording it \
                 was playing — the decoder runs on and its last frame stands over \
                 the new content"
        );
    }

    // ② and a surface that has been given nothing yet keeps its seat: the
    // tear-off, where the window has the engine and the picture is one call
    // behind it.
    assert_eq!(
        surface_subject_of(Some(&PreviewPane::default()), false),
        SurfaceSubject::Unfilled
    );
    assert!(surface_subject_of(None, false).is_still(&recording));
    assert!(surface_subject_of(Some(&PreviewPane::default()), false).is_still(&recording));

    // ③ the seat and its decoder really go, on each of the three surfaces.
    let before = engines_outstanding();
    let mut seats = video_seat::VideoSeats::default();
    let surfaces = [
        PreviewSurface::Seat(LeafId {
            tab: TabId(1),
            seat: SeatId(2),
        }),
        PreviewSurface::Float(9),
        PreviewSurface::Peek,
    ];
    let now = Instant::now();
    for surface in surfaces {
        seats
            .open(surface, &recording, now)
            .unwrap_or_else(|error| panic!("{surface:?} opens the fixture: {error:?}"));
    }
    assert_eq!(engines_settling_to(before + 3), before + 3);
    for (surface, pane) in
        surfaces
            .into_iter()
            .zip([&after_a_document, &after_a_picture, &after_another])
    {
        let seat = seats.get(surface).expect("the seat is still there");
        let subject = surface_subject_of(Some(pane), false);
        assert!(!subject.is_still(seat.path()));
        // Which is what the sweep does with that answer.
        assert!(seats.close(surface), "{surface:?} gives its seat up");
        assert!(seats.get(surface).is_none(), "{surface:?}");
    }
    assert!(seats.is_empty(), "no surface is left holding a recording");
    assert_eq!(
        engines_settling_to(before),
        before,
        "a seat that was swept away left its decoder running"
    );

    // And the sweep asks that question rather than the picture lane's.
    let sweep = method_body("Runtime", "sweep_video_seats");
    assert!(
        sweep.contains("self.preview_subject(*surface)"),
        "the sweep no longer asks what the surface is showing across every \
             lane, so a lane that is not the picture lane reads as silence"
    );
    assert!(
        !sweep.contains("self.preview_picture("),
        "the sweep still reads the picture lane, which is the reading that \
             cannot tell a markdown file from a tear-off"
    );
}

/// RED — **two recordings trading places keep both engines** (§7.44 ⑮,
/// 2026-08-30).
///
/// The half of "a recording follows its pane" that a loop of
/// [`video_seat::VideoSeats::rehome`] cannot do. A pane dropped on the
/// *centre* of another tab's pane trades: two journeys in one gesture, and
/// the second one's destination is the first one's origin. Rehomed one at a
/// time, the second journey arrives at a surface the first has just filled
/// and the shutdown that `open` and `rehome` both owe a surface takes a live
/// decoder with it — on the glass, one of the two videos goes black while
/// the pane around it arrives intact.
///
/// So the carrier lifts every travelling recording off its surface before it
/// puts any of them down, which is what
/// [`video_seat::VideoSeats::take`] and `put` are for, and this is that
/// sequence run over two real engines.
///
/// RED GATE: replace the two `take`s and two `put`s with two `rehome`s in
/// the order the gesture produces them and `engines_shut_down` moves by one
/// while the second surface comes back holding the first one's texture.
#[test]
fn two_recordings_trading_places_keep_both_engines() {
    use bt_platform::video::engine::{engines_outstanding, engines_shut_down};
    let _ledger = ledger_gate();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/assets/folio-video-test.mp4");
    let before = engines_outstanding();
    let mut seats = video_seat::VideoSeats::default();
    let now = Instant::now();
    let travelling = PreviewSurface::Seat(LeafId {
        tab: TabId(1),
        seat: SeatId(2),
    });
    let standing = PreviewSurface::Seat(LeafId {
        tab: TabId(4),
        seat: SeatId(1),
    });
    for surface in [travelling, standing] {
        seats
            .open(surface, &fixture, now)
            .unwrap_or_else(|error| panic!("{surface:?} opens the fixture: {error:?}"));
    }
    assert_eq!(
        engines_settling_to(before + 2),
        before + 2,
        "two surfaces are two decoders"
    );
    let keys: Vec<String> = [travelling, standing]
        .iter()
        .map(|surface| {
            seats
                .get(*surface)
                .expect("both surfaces hold a seat")
                .key()
                .to_owned()
        })
        .collect();
    let stopped = engines_shut_down();

    // The trade, as the carrier runs it: both off their surfaces first.
    let lifted: Vec<(PreviewSurface, video_seat::VideoSeat)> =
        [(travelling, standing), (standing, travelling)]
            .iter()
            .filter_map(|(was, to)| Some((*to, seats.take(*was)?)))
            .collect();
    for (to, seat) in lifted {
        seats.put(to, seat);
    }

    assert_eq!(
        engines_shut_down(),
        stopped,
        "a trade shut an engine down, which is one of the two pictures going \
             black"
    );
    assert_eq!(engines_outstanding(), before + 2, "and both are still open");
    assert_eq!(
        seats.get(standing).expect("the traveller arrived").key(),
        keys[0],
        "the traveller kept its own texture"
    );
    assert_eq!(
        seats
            .get(travelling)
            .expect("and the pane it traded with went the other way")
            .key(),
        keys[1],
        "each recording is the one its pane was showing"
    );
    seats.shutdown_all();
    assert_eq!(engines_settling_to(before), before);
}

/// RED — **the still and the first played frame land in the same rectangle**
/// (user ruling 2026-08-28: *「播放前后同几何」*; §7.44 ①, closing §7.42 ⑤ for
/// all three surfaces).
///
/// §7.42 pinned the *rule* — one `video_fit_extent` for both — as a property
/// of two pure functions. What it could not pin, because slice ① had one
/// surface and no play verb on the other two, is that the two callers hand
/// those functions the same box. This does:
/// [`video_still_destination`] is what a paused surface's frame is drawn by
/// and [`bt_render::video_frame_rect`] is what the playing layer is drawn
/// by, and they are asked here about the same body and the same recording.
///
/// **The rounding is asserted to the pixel and not to a tolerance.** A
/// disagreement of one pixel is exactly what a reader sees as a flicker when
/// they press play, and a test that allowed one would be a test written to
/// pass.
///
/// RED GATE: fit the still with `bt_render::preview_image_extent` — the
/// picture channel's rule, which never enlarges — and every small-recording
/// row fails by hundreds of pixels. That is the `next12` defect exactly: a
/// 160×120 clip drawn 160×120 in a full-height pane, jumping to fill it the
/// instant the first frame arrived.
#[test]
fn the_still_and_the_first_played_frame_share_a_rect() {
    let bodies = [
        [0.0_f32, 0.0, 960.0, 556.0],
        [120.0, 48.0, 1_200.0, 800.0],
        [40.0, 40.0, 320.0, 220.0],
    ];
    let sources = [[160_u32, 120_u32], [1_920, 1_080], [1_080, 1_920]];
    for body in bodies {
        for source in sources {
            let still = video_still_destination(body, source);
            let box_ = viewport_of_rect(body).expect("a real body");
            let played =
                bt_render::video_frame_rect(box_, source[0], source[1]).expect("a real recording");
            for axis in 0..4 {
                assert_eq!(
                    still[axis].round(),
                    played[axis].round(),
                    "{body:?} {source:?}: the still is {still:?} and the frame is {played:?}"
                );
            }
        }
    }
}

/// PIN (user ruling 2026-08-23, 「一个名字只该有一个含义」;
/// `docs/DESIGN.md` §7.10 ⑥) — **every door that lands a source opens a page
/// as a page.**
///
/// This is the ticket's 正题 and the cost slice ⑤ named when it left the
/// account open: teaching `preview_ftype` to answer `Web` by name turns every
/// `PreviewSource::File` whose name is a page into a buffer the document pool
/// has no reader for. The answer is not to soften the classification, it is
/// that **no such buffer is ever landed** — the pool's own door turns it back
/// onto the engine's lane, and the pool's own door is the one every document
/// arrives by.
///
/// The four sources below are the four the ticket names, built the way their
/// doors build them: a drop and a files-column double click hand a path
/// straight to `PreviewSource::file`; a switcher row hands back the pool's
/// own identity; a session line and a Recent seed come through
/// [`preview_source_of`] and [`preview_source_of_recent`]. One predicate
/// answers for all four because they are all one question.
///
/// RED GATE: delete the `source_opens_as_a_page` arm from
/// `Runtime::open_preview_source_on` — the source pin at the end of this test
/// fails, and on the machine a dropped `.html` draws the "no preview for this
/// file type" card that this ruling exists to abolish.
#[test]
fn every_door_that_lands_a_source_opens_a_page_as_a_page() {
    let page = r"D:\site\index.html";
    let pdf = r"D:\reports\report.pdf";
    let document = r"D:\notes\notes.md";
    // ① a drop on a pane, and ② a double click in the files column: both
    // hand a path to `PreviewSource::file` and nothing else.
    for target in [page, pdf] {
        assert_eq!(
            source_opens_as_a_page(&preview::PreviewSource::file(target)),
            Some(PathBuf::from(target)),
            "a dropped or double-clicked page is a page: {target}"
        );
    }
    assert_eq!(
        source_opens_as_a_page(&preview::PreviewSource::file(document)),
        None,
        "and a document is untouched, which is the regression half"
    );
    // ③ a switcher row, which hands back the pool's own identity — including
    // a legacy row an older build wrote while `.html` was still text.
    assert_eq!(
        source_opens_as_a_page(&preview::PreviewSource::file(page)),
        Some(PathBuf::from(page))
    );
    // ④ a `session.json` line and ⑤ a Recent seed, through the one pair that
    // reads a stored row back into an identity.
    assert_eq!(
        source_opens_as_a_page(&preview_source_of(page, bt_persist::PreviewSourceV1::File)),
        Some(PathBuf::from(page)),
        "a restored `cur` naming a page is a page"
    );
    assert_eq!(
        source_opens_as_a_page(&preview_source_of_recent(
            &bt_persist::RecentPreviewV1::File(page.to_owned())
        )),
        Some(PathBuf::from(page)),
        "and so is a Recent seed"
    );
    // A page already stored as a page is not this predicate's business — it
    // never reaches the document pool at all — and a composed document is
    // not a page however its path inside a repository is spelled.
    assert_eq!(
        source_opens_as_a_page(&preview::PreviewSource::Web(
            "file:///D:/site/index.html".to_owned()
        )),
        None
    );
    assert_eq!(
        source_opens_as_a_page(&preview::PreviewSource::GitDiff {
            root: PathBuf::from(r"D:\repo"),
            path: "design/ui-mockup.html".to_owned(),
            against: preview::GitDiffAgainst::WorkingTree,
        }),
        None,
        "a reading of a repository has no file, so it is not a page"
    );
    // A share is not a page, so it keeps the document lane's network card —
    // the one refusal §7.1.3 has always shown for it.
    assert_eq!(
        source_opens_as_a_page(&preview::PreviewSource::file(r"\\server\share\index.html")),
        None
    );

    // **And the door really asks.** The predicate above is worth nothing if
    // the one function every document lands through does not consult it, and
    // that is a fact about this file rather than about a value — the same
    // reason `both_preview_openers_write_a_line_when_nothing_opens` reads the
    // source it is about.
    let door = method_body("Runtime", "open_preview_source_on");
    assert!(
        door.contains("source_opens_as_a_page(&source)")
            && door.contains("self.open_preview_web_file_on(surface, path)"),
        "the pool's own door does not turn a page-named source back onto the \
             engine's lane, so a `.html` can still land as a document:\n{door}"
    );
    // **And the one door that changes a name without landing a source.** A
    // rename re-asks `preview_ftype` on purpose — `notes.md` → `notes.txt`
    // really does turn a rendered document into a text editor — so since a
    // name can say *page*, `notes.md` → `notes.html` has to reach the engine
    // rather than leave a buffer full of text drawn as a kind this window
    // cannot read.
    let rename = method_body("Runtime", "rename_preview_file");
    assert!(
        rename.contains("source_opens_as_a_page(&preview::PreviewSource::file(&new))")
            && rename.contains("self.open_preview_web_file_on(surface, path)"),
        "a rename into a page's name leaves the file on the document lane:\n{rename}"
    );
}
