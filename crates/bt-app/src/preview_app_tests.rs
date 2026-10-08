//! **`preview`, as the application drives it.** Tests whose first assertion is about
//! `preview`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    A, NO_MODIFIERS, PtyPresentationHarness, TAB_ONE, arriving_as, buffer_read_from, buffer_saying,
    card_text, cross_tab, disk_scratch, document_key, found, frame_row_text, markdown_body,
    method_body, move_the_disk_forward, one_picture, press, rested_bars, row_box, ruler, seat_of,
    source, tab_with_a_preview, text_buffer,
};
use bt_source::{Pattern, View, needle};
use winit::keyboard::{Key, NamedKey};

/// PIN (user ruling 2026-08-25, B5) — **the breadcrumb's last segment is the
/// same editor the head's name is, and the files column grows a `Rename` row
/// that is a third door onto it.**
///
/// One machine and three doors, which is this window's standing rule for any
/// verb that gains a second way in: the crumb rail and the tree row seed
/// [`TabRename`] exactly as the head does — the whole name in the box, the
/// stem selected, the suffix left standing — and all of them commit through
/// the one function that calls `std::fs::rename`. Three seeds would be three
/// chances for a name to be moved on disk by a rule nobody wrote down twice.
///
/// Red gate: seed either new door by hand and the stem assertions go red;
/// give either its own commit and the routing assertions name the door that
/// went its own way.
#[test]
fn the_crumb_and_the_tree_row_open_the_head_s_own_rename() {
    let body = |name: &str| method_body("Runtime", name);
    let surface = seat_of(TAB_ONE, SeatId(7));
    let source = preview::PreviewSource::file(r"C:\notes\notes.md");
    let crumb = TabRename::open_crumb(surface, source.clone(), "notes.md");
    assert_eq!(crumb.text(), "notes.md", "the whole name is in the box");
    assert_eq!(
        crumb.selection(),
        0..5,
        "the selection stops before the dot"
    );
    assert_eq!(
        crumb.caret(),
        crumb.selection().end,
        "and the caret stands at the end of the selection"
    );

    let row = TabRename::open_files_row(
        LeafId {
            tab: TAB_ONE,
            seat: SeatId(3),
        },
        "/notes/notes.md",
        "notes.md",
    );
    assert_eq!(row.text(), "notes.md");
    assert_eq!(row.selection(), 0..5, "a tree row is seeded the same way");
    assert_eq!(row.caret(), row.selection().end);

    assert!(
        body("finish_rename").contains("RenameSubject::PreviewCrumb"),
        "the crumb's draft is committed by the one exit every draft leaves by"
    );
    assert!(
        body("finish_rename").contains("RenameSubject::FilesRow"),
        "and so is a tree row's"
    );
    assert!(
        body("run_file_menu_row").contains("self.open_files_row_rename("),
        "the menu row opens the editor rather than asking a dialog for a name"
    );
}

/// PIN (user ruling 2026-08-19) — **a file's box opens with its STEM
/// selected and its suffix not.**
///
/// A tab's name is an override, so its box holds your name and all of it is
/// selected; a file has no name under its name, so the box holds the whole
/// of it and only the part you almost always mean to replace is selected.
/// Typing over `notes.md` leaves `.md` standing.
///
/// `.gitignore` is the case that makes the rule a rule rather than a
/// `rfind('.')`: it is a name, not an empty stem with a suffix, so the whole
/// of it is selected.
///
/// **AND THE CARET IS THE SELECTION'S OWN END** (user report on a real
/// window, 2026-08-20, screenshot `[Cargo].toml|`). The box opened with
/// `Cargo` washed and the hairline standing after `.toml`, which is one
/// editor saying two different things about where typing will land — and the
/// verbs were never in any doubt, because every one of them reads `selected`
/// and not `caret`. Only the drawn caret and the IME's candidate anchor lied.
/// This is the invariant [`TabRename`] is built on written as an assertion:
/// **when there is a selection, the caret IS its far end.** `.gitignore`
/// selects the whole draft, so for that name the far end is the end of the
/// text — the same rule, not an exception to it.
///
/// Red gate: select the whole draft and the first assertion goes red three
/// out; drop the `> 0` filter and `.gitignore` selects nothing at all; seed
/// `caret: text.len()` again and the two `caret == selected` assertions go
/// red on every name that has a suffix.
#[test]
fn a_files_box_opens_with_its_stem_selected() {
    let surface = seat_of(TAB_ONE, SeatId(7));
    let source = preview::PreviewSource::file(r"C:\notes\notes.md");
    let editor = TabRename::open_file(surface, source.clone(), "notes.md");
    assert_eq!(editor.text(), "notes.md", "the whole name is in the box");
    assert_eq!(
        editor.selection(),
        0..5,
        "the selection stops before the dot"
    );
    assert_eq!(
        editor.caret(),
        editor.selection().end,
        "and the caret stands at the end of the selection, not at the end of \
             the draft"
    );
    assert_eq!(editor.tab(), None, "a file is not a tab");

    let dotfile = TabRename::open_file(surface, source.clone(), ".gitignore");
    assert_eq!(
        dotfile.selection(),
        0..".gitignore".len(),
        "a leading dot is part of the name, not the start of a suffix"
    );
    assert_eq!(
        dotfile.caret(),
        dotfile.selection().end,
        "and its caret is at that selection's end, which for a whole-draft \
             selection is the end of the text"
    );
    assert_eq!(
        dotfile.caret(),
        ".gitignore".len(),
        "— so this one, and only this one, opens with its caret at the end \
             of the name"
    );

    // Typing replaces the stem and leaves the suffix, which is the whole
    // point of the offset above.
    let mut typing = TabRename::open_file(surface, source, "notes.md");
    typing.insert("todo");
    assert_eq!(typing.text(), "todo.md");
    assert_eq!(typing.caret(), 4);
    // And `→` out of the selection lands at its end rather than at the end
    // of the draft, which is what a prefix selection means.
    let mut walked = TabRename::open_file(
        seat_of(TAB_ONE, SeatId(7)),
        preview::PreviewSource::file(r"C:\notes\notes.md"),
        "notes.md",
    );
    press(&mut walked, &Key::Named(NamedKey::ArrowRight), NO_MODIFIERS);
    assert_eq!(
        walked.caret(),
        5,
        "the near edge of the selection is its end"
    );
}

/// PIN (user ruling 2026-08-20) — **`Ctrl+A` selects the whole draft, and it
/// is the one chord this editor keeps.**
///
/// The stem selection stays (ruling five), so the suffix needs a way back —
/// and until today the sentence promising one ("the suffix is one `Ctrl+A`
/// gesture away") described a gesture that did not exist: every Ctrl chord
/// was swallowed unhandled.
///
/// **One editor, not two**: the same chord answers on a tab, where the draft
/// is already whole-selected and pressing it is therefore idempotent. A
/// select-all that only worked on one of the two subjects would be two
/// editors wearing one type.
///
/// It stays out of `shortcuts::BINDINGS` for [`graph_key_of`]'s stated
/// reason, which `preview_edit::command`'s own `"a" => SelectAll` already
/// follows: that table is the registry of chords this window *claims from a
/// shell*, and while the editor holds the keyboard there is no shell
/// listening. It is a key inside a text field, like the six the graph
/// answers and the arrows the files column answers.
///
/// Red gate: take the `Ctrl+A` arm out of `rename_key` and the chord falls
/// back into the swallow-every-chord arm, so the file's selection stays at
/// the stem and the typing assertion writes `todo.md` instead of `todo`.
#[test]
fn ctrl_a_selects_the_whole_draft() {
    let ctrl = ModifiersState::CONTROL;
    let a = || Key::Character("a".into());

    let mut file = TabRename::open_file(
        seat_of(TAB_ONE, SeatId(7)),
        preview::PreviewSource::file(r"C:\notes\notes.md"),
        "notes.md",
    );
    assert_eq!(file.selection(), 0..5, "it opens on the stem");
    assert_eq!(
        press(&mut file, &a(), ctrl),
        RenameVerdict::Held,
        "the chord is the editor's own and never leaves it"
    );
    assert_eq!(file.selection(), 0..8, "and now the whole name is selected");
    assert_eq!(
        file.caret(),
        file.selection().end,
        "the caret is still the selection's far end"
    );
    file.insert("todo");
    assert_eq!(
        file.text(),
        "todo",
        "so typing replaces the suffix too, which is the point of the chord"
    );

    // The tab's draft opens whole-selected, so the chord is idempotent
    // there — the same verb, arriving at a fixed point.
    let mut tab = TabRename::open(A, Some("build"));
    assert_eq!((tab.selection(), tab.caret()), (0..5, 5));
    assert_eq!(press(&mut tab, &a(), ctrl), RenameVerdict::Held);
    assert_eq!(
        (tab.selection(), tab.caret()),
        (0..5, 5),
        "pressing it on a whole selection changes nothing"
    );

    // After the selection has been collapsed by a verb, the chord brings it
    // back over everything the draft holds now, not over what it held then.
    let mut typed = TabRename::open(A, None);
    typed.insert("release");
    assert!(typed.selection().is_empty(), "nothing selected");
    assert_eq!(typed.caret(), 7);
    assert_eq!(press(&mut typed, &a(), ctrl), RenameVerdict::Held);
    assert_eq!((typed.selection(), typed.caret()), (0..7, 7));

    // **AltGr is typing, not a chord** — it arrives as Ctrl+Alt, and the
    // door opened here must not swallow the `a` a Polish or German layout
    // is spelling with it. This editor swallows Ctrl+Alt as it always has;
    // what matters is that the select-all arm is not what does it.
    let mut altgr = TabRename::open_file(
        seat_of(TAB_ONE, SeatId(7)),
        preview::PreviewSource::file(r"C:\notes\notes.md"),
        "notes.md",
    );
    assert_eq!(
        press(
            &mut altgr,
            &a(),
            ModifiersState::CONTROL | ModifiersState::ALT
        ),
        RenameVerdict::Held
    );
    assert_eq!(
        altgr.selection(),
        0..5,
        "Ctrl+Alt is not the select-all chord and left the stem selection alone"
    );
}

/// PIN — **the head's `↗` hands over the seat's own file, and only when that
/// file is a page** (user ruling 2026-08-20, §7.1.5g).
///
/// One predicate answers both halves of the button: whether it is drawn at
/// all, and which path it hands over when it is pressed. A second spelling
/// of "is this a page" would be a button that can come to be lit over a file
/// it will not open — the class of bug D4 exists to make impossible — and a
/// second spelling of "which file" would be a button that hands over
/// whatever the *focused* seat is showing rather than the one it stands on.
///
/// **And the page it asks about is the page class, not one spelling of it**
/// (user ruling 2026-08-23, second ruling of the day; §7.10 ⑥). The class
/// was settled that morning — `.html`, `.htm` and `.pdf` open on one lane
/// from every door — and the arrow was still reading the older, narrower
/// predicate, so one class had two heads: a local `.html` page wore the
/// arrow and a local `.pdf` page did not. The `.pdf` is the member that
/// needs it most, because the browser's reader prints, rotates and
/// annotates where the embedded one does not, and because `</>` is no
/// second door for it — there is nothing in a PDF for developer tools to
/// show.
///
/// MUTATIONS:
/// ① drop the page filter — every preview head in the window grows an
///    arrow, and `notes.md` gets handed to a browser;
/// ② answer from the buffer's `name` instead of `source.file_path()` — a git
///    diff of `index.html` grows an arrow over a document that has no file
///    at all, and the press has nothing to give the shell;
/// ③ spell the extension test with `ends_with` — `report.html.txt`, a text
///    file, is handed to a browser.
///
/// **④ (2026-08-23) drop the `web_url` arm** and the arrow goes out the
/// moment a local page starts *rendering* instead of showing its source —
/// which, since the ruling of that day, is always. The seat would then have
/// no way to the page's own front at all, which is §7.1.5g ②″'s complaint
/// with the two views swapped.
///
/// **⑤ (2026-08-23) narrow the filter back to the HTML-only predicate** on
/// either arm and every `.pdf` assertion below fails: the class the window
/// opens on one lane would be wearing two different heads again.
#[test]
fn only_a_page_with_a_file_behind_it_is_handed_over_to_a_browser() {
    for name in [
        r"D:\Developer\folio-terminal\design\ui-mockup.html",
        r"C:\Users\me\TIMELINE.HTM",
        r"D:\中文\页.html",
        r"D:\reports\report.pdf",
        r"C:\Users\me\MANUAL.PDF",
        r"D:\中文\说明书.pdf",
    ] {
        let source = preview::PreviewSource::file(name);
        assert_eq!(
            preview_page_hand_off(&source),
            Some(PathBuf::from(name)),
            "a page hands over its own path: {name:?}"
        );
    }
    // **And the same file while the engine is drawing it.** A local page's
    // identity is the `file:` URL this window minted, so the path is taken
    // back off it by the one function that reads such a URL — and a page
    // that is not a local file has nothing to hand anybody.
    for (url, path) in [
        (
            "file:///D:/Developer/folio-terminal/design/ui-mockup.html",
            r"D:\Developer\folio-terminal\design\ui-mockup.html",
        ),
        (
            "file:///C:/Program%20Files/report.htm",
            r"C:\Program Files\report.htm",
        ),
        ("file:///D:/reports/report.pdf", r"D:\reports\report.pdf"),
    ] {
        assert_eq!(
            preview_page_hand_off(&preview::PreviewSource::Web(url.to_owned())),
            Some(PathBuf::from(path)),
            "a rendered local page hands over the file under it: {url:?}"
        );
    }
    // **A remote page still has nothing to hand over, and that is not the
    // narrow reading coming back** — it is the arrow asking for a *file*.
    // The address is handed over by the foot (§7.7 ③), so a `.pdf` served
    // over `https` answers here exactly as `.html` does.
    for url in [
        "http://localhost:5173/app",
        "https://example.test/index.html",
        "https://example.test/manual.pdf",
        "file://server/share/page.html",
        "file://server/share/report.pdf",
    ] {
        assert_eq!(
            preview_page_hand_off(&preview::PreviewSource::Web(url.to_owned())),
            None,
            "a page with no local file behind it has nothing to hand over: {url:?}"
        );
    }
    // Everything else the seat can show. The extension is the real one and
    // never a substring of the name, which is `path_opens_as_a_page`'s own
    // rule reaching this button rather than a second reading of it.
    for name in [
        r"C:\Users\me\notes.md",
        r"C:\Users\me\a.png",
        r"C:\site\index.htmlx",
        r"C:\site\report.html.txt",
        r"C:\site\report.pdfx",
        r"C:\site\notes.pdf.txt",
        r"C:\site\html",
        r"C:\site\pdf",
    ] {
        assert_eq!(
            preview_page_hand_off(&preview::PreviewSource::file(name)),
            None,
            "not a page, so there is no arrow and nothing to hand over: {name:?}"
        );
    }
    // A document this window composed out of a repository has no file, so it
    // has no door out however its path inside the repo is spelled.
    assert_eq!(
        preview_page_hand_off(&preview::PreviewSource::GitDiff {
            root: PathBuf::from(r"D:\repo"),
            path: "design/ui-mockup.html".to_owned(),
            against: preview::GitDiffAgainst::WorkingTree,
        }),
        None,
        "a composed document is not a file, so it is not a page either"
    );
}

/// RED (39) — **A `file:` address on another machine's share is handed over as a share, and no
/// `file:` address ever leaves as a bare address.**
///
/// A share takes the door ticket 14's `HyperlinkActivation::Share` takes
/// (`Runtime::open_unverified_reference`, which reads the program list). A `file:` address this
/// machine names no share from — a distribution's share, a POSIX host on a Mac — leaves by no
/// road: a path never leaves as a URI. The expected path is the real `bt_platform::file_uri_to_path`
/// answer on this platform, held to `is_a_share_on_another_machine`.
///
/// MUTATION: delete the `file:` fork in `preview_page_browser_hand_off` — the share leaves as
/// `PageHandOff::Address("file://server/…")`.
#[test]
fn a_file_address_on_a_share_is_handed_over_as_a_share() {
    for url in [
        "file://server/share/page.html",
        "file://server/share/run.cmd",
    ] {
        let answer = preview_page_browser_hand_off(&preview::PreviewSource::Web(url.to_owned()));
        match bt_platform::file_uri_to_path(url)
            .filter(|path| bt_transcript::paths::is_a_share_on_another_machine(path))
        {
            Some(share) => assert_eq!(answer, Some(PageHandOff::Share(share)), "{url:?}"),
            None => assert_eq!(
                answer, None,
                "no share here, and no address either: {url:?}"
            ),
        }
    }
    assert_eq!(
        preview_page_browser_hand_off(&preview::PreviewSource::Web(
            "file://wsl.localhost/Debian/etc/hosts".to_owned()
        )),
        None,
        "a file: address that is neither a local file nor a share is not handed to the shell"
    );
}

#[test]
fn open_synchronized_update_still_suppresses_its_intermediate_state() {
    let mut harness = PtyPresentationHarness::new(24, 2);
    assert!(harness.feed_drain(b"base"));
    assert!(harness.present_pending());

    assert!(!harness.feed_drain(b"\x1b[?2026h\rhidden-intermediate"));
    assert!(harness.session.synchronized_update_deadline().is_some());
    assert_eq!(harness.publications, 1);
    assert!(harness.pending.pending_frame().is_none());
    assert!(frame_row_text(harness.last_presented.as_ref().unwrap(), 0).contains("base"));

    assert!(harness.feed_drain(b"\x1b[?2026l"));
    assert!(harness.session.synchronized_update_deadline().is_none());
    assert_eq!(harness.publications, 2);
    assert!(
        frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("hidden-intermediate")
    );
}

// ── slice 2: the read-only view family ──────────────────────────────

/// PIN — P108, at the rectangle. **Only additions and deletions are
/// tinted, and every tint is the width of the body.**
///
/// The mock-up's own comment names the double bug this pins: half-width
/// tints, from a band derived out of the text, and a mid-pane scrollbar. A
/// diff read at a horizontal scroll is where both show, so the assertion is
/// made at one.
///
/// Mutation: tint `Context` as well, or build the band from `row_rect`
/// instead of `band_rect`.
#[test]
fn only_the_changed_lines_of_a_diff_are_tinted_and_the_tints_are_full_width() {
    let palette = bt_render::chrome_palette();
    let body = [40.0, 100.0, 440.0, 400.0];
    let metrics = seats::preview_diff_metrics(1.0);
    let source = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-was\n+now\n keep\n";
    let mut top = 0.0_f32;
    let rows: Vec<DiffRow> = source
        .lines()
        .map(|line| {
            let kind = preview::diff_line_kind(line);
            if kind == preview::DiffLineKind::Hunk {
                top += 8.0;
            }
            let row = DiffRow {
                text: line.to_owned(),
                kind,
                top,
            };
            top += metrics.line_height;
            row
        })
        .collect();
    let geometry = seats::preview_mono_geometry(body, metrics, top, 400, 8.0, [260.0, 0.0]);
    let built = build_preview_diff_body(&geometry, &rows, &palette);

    assert_eq!(built.quads.len(), 2, "one deletion and one addition");
    for quad in &built.quads {
        assert_eq!(
            [quad.rect[0], quad.rect[2]],
            [body[0], body[2]],
            "a tint is the body's width even at 260px of horizontal scroll"
        );
    }
    assert_eq!(built.quads[0].color, palette.preview_diff_del);
    assert_eq!(built.quads[1].color, palette.preview_diff_add);

    // And the inks: the header is quiet, the hunk marker is the accent, the
    // rest is body text.
    let ink = |index: usize| built.paragraphs[index].runs[0].color;
    assert_eq!(ink(0), palette.files_row_muted, "diff --git");
    assert_eq!(ink(1), palette.files_row_muted, "---");
    assert_eq!(ink(2), palette.files_row_muted, "+++");
    assert_eq!(ink(3), palette.preview_diff_hunk, "@@");
    assert_eq!(ink(4), palette.preview_body_text, "-was");
    assert_eq!(ink(6), palette.preview_body_text, " keep");
    // `.dhunk { margin-top: 8px }` pushes the hunk and everything under it.
    assert_eq!(
        built.paragraphs[3].rect[1] - built.paragraphs[2].rect[1],
        metrics.line_height + 8.0
    );
}

/// PIN (user report, 2026-08-13: "整屏空白") — **a block paints every pixel
/// it reserved, and the rows it paints are the rows it was measured for.**
///
/// The reported hole. A list was measured with the shaper — each item asked
/// how many lines it took when *wrapped* into the pane — and then drawn on
/// the assumption that every item took exactly one. In `docs/DESIGN.md`, read
/// in a pane narrow enough that its long items wrap three and four ways, the
/// two answers differ by most of a screen, and the difference came out as
/// blank: the block reserved what the measurement asked for, painted a third
/// of it, and the next heading began below the reservation. Its quieter twin
/// was that every wrapped item's second line was clipped by the same
/// one-line box, which is text going missing rather than space appearing.
///
/// Stated as the property and not as the document: **the n-th row starts
/// where the first n-1 rows ended, and the last one ends where the block
/// does.** That is true of a list, a quote and a table alike, which is why
/// all three are asserted here through the one painter.
///
/// MUTATION: give the list arm of `build_preview_markdown_body` back its
/// `let item_height = metrics.line_height;` and the second item's box is a
/// line tall in a three-line hole — every assertion below the first goes red.
#[test]
fn a_wrapped_row_is_painted_in_the_box_it_was_measured_for() {
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 600.0, 4000.0];
    let metrics = seats::preview_markdown_metrics(1.0);
    let line = metrics.line_height;
    let origin = body[1] + metrics.padding_y;

    // Three items that wrap to one, three and two lines. Uneven on purpose:
    // equal rows would let the buggy painter answer right by accident.
    //
    // **The rows carry `li + li { margin-top: .25em }` since 2026-08-16**
    // (§7.1.3i): the gap lives in the *top* of every row after the first,
    // exactly as `measure_markdown_block` now reserves it, so the property
    // being asserted is unchanged — the box is what the measurement asked
    // for — and the text inside it starts below its own margin.
    let blocks = preview::parse_markdown("- one\n- two\n- three\n");
    let gap = metrics.list_item_gap;
    let heights = vec![line, gap + line * 3.0, gap + line * 2.0];
    let layout: preview_viewport::Layout =
        vec![MarkdownBlockLayout::rows(heights.clone(), 0.0)].into();
    let built = markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        (&blocks, &layout),
        &palette,
    );
    let items: Vec<_> = built
        .paragraphs
        .iter()
        .filter(|p| {
            p.runs
                .first()
                .is_some_and(|run| run.text.starts_with('\u{2022}'))
        })
        .collect();
    assert_eq!(items.len(), 3, "one paragraph per item");
    let mut expected_top = origin;
    for (index, item) in items.iter().enumerate() {
        let margin = if index == 0 { 0.0 } else { gap };
        assert_eq!(
            item.rect[1],
            expected_top + margin,
            "item {index} starts where the items above it ended, plus its own \
                 `li + li` margin"
        );
        assert_eq!(
            item.rect[3] - expected_top,
            heights[index],
            "item {index} is given the box its own wrap was measured into"
        );
        expected_top = item.rect[3];
    }
    assert_eq!(
        expected_top,
        origin + layout[0].height,
        "and the last row ends exactly where the block does — no hole under it"
    );

    // The same property for a quote, whose rows sit inside the block's own
    // padding, and whose accent bar spans the whole of it rather than one
    // bar per line.
    // A bare `>` between them, because two *source* lines of a quote are one
    // quoted paragraph since the CommonMark ruling of 2026-08-13 — what this
    // assertion is about is a quote with two entries, and that is how one is
    // now written.
    let quoted = preview::parse_markdown("> a\n>\n> b\n");
    let quote_rows = vec![line * 2.0, line];
    let quote_layout: preview_viewport::Layout = vec![MarkdownBlockLayout::rows(
        quote_rows.clone(),
        metrics.quote_padding_y * 2.0,
    )]
    .into();
    let built = markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        (&quoted, &quote_layout),
        &palette,
    );
    let bars: Vec<_> = built
        .quads
        .iter()
        .filter(|quad| quad.color == palette.accent)
        .collect();
    assert_eq!(bars.len(), 1, "one bar for the quote, not one per line");
    assert_eq!(
        bars[0].rect[3] - bars[0].rect[1],
        quote_layout[0].height,
        "and it is as tall as the block it marks"
    );
    assert_eq!(bars[0].rect[2] - bars[0].rect[0], metrics.quote_bar);
    let lines: Vec<_> = built
        .paragraphs
        .iter()
        .filter(|p| p.rect[0] > bars[0].rect[2])
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].rect[1], origin + metrics.quote_padding_y);
    assert_eq!(
        lines[1].rect[1],
        lines[0].rect[1] + quote_rows[0],
        "the second quoted line clears the first one's own wrap"
    );
}

/// PIN — `docs/DESIGN.md`, the file the report was made against, parses into
/// blocks that reserve nothing they cannot paint.
///
/// The fixture is the real file rather than a sample of it, because two of
/// the three hypotheses for the hole were about the *document* and not about
/// the painter: a fence left open by an indented closer would swallow the
/// rest of the file into one enormous `Code` block, and a pipe row measured
/// as prose would be a paragraph the width of a table. Both are refuted here
/// by construction, and stay refuted if someone edits the document.
///
/// PIN — **the glance card's document is the preview pane's document**, kind
/// for kind and shape for shape (user ruling, 2026-08-13).
///
/// The card asks [`preview::preview_view`] which body a file is, exactly as a
/// pane does, and then goes down the same builders. So the claim to pin is
/// that those two steps, run at the card's own width, produce the pane's own
/// shapes: a csv is a *table* — cells with a heading band and a grid — and a
/// patch is three inks with bands under two of them. Neither is a list of
/// lines, which is what the card drew before the ruling and what a regression
/// would silently go back to.
///
/// The height each one comes out at is asserted beside it, because that is
/// the number the card is sized by: a two-row table must be a shorter card
/// than a twenty-row one, or "the card shrink-wraps its body" has stopped
/// being true the moment the body stopped being lines.
///
/// MUTATION: give the card a `PreviewView::Text` document for every file (the
/// plain-text body this replaced) — the table assertions lose their grid and
/// their heading band, and the diff's two tints collapse to none.
#[test]
fn the_glance_cards_document_is_the_panes_own_table_and_its_own_three_inks() {
    let scale = 1.0_f32;
    let palette = bt_render::chrome_palette();
    let advance = 7.0_f32;
    let body = [
        0.0,
        0.0,
        file_peek::body_width(scale),
        file_peek::body_max_height(scale, true),
    ];

    // ① A csv is a table on both surfaces, because one predicate answers for
    //    both.
    assert_eq!(
        preview::preview_view("rows.csv", preview::preview_ftype("rows.csv"), false),
        preview::PreviewView::Table
    );
    let csv = "name,size\nalpha,10\nbeta,20\n";
    let rows = preview::csv_rows(csv);
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let column_cells: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| bt_unicode::text_width(cell))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let table = PreviewDocument::Table {
        rows: rows.clone(),
        column_cells: column_cells.clone(),
    };
    let geometry =
        seats::preview_table_geometry(body, &column_cells, rows.len(), advance, scale, [0.0, 0.0]);
    let drawn = build_preview_table_body(&geometry, &rows, &palette);
    assert_eq!(
        drawn.paragraphs.len(),
        rows.len() * columns,
        "one paragraph per cell — a table, not three lines of commas"
    );
    assert!(
        drawn
            .paragraphs
            .iter()
            .any(|cell| cell.runs.iter().any(|run| run.text == "alpha")),
        "and the cells hold the file's own words"
    );
    assert!(
        drawn.quads.len() > rows.len(),
        "with a grid under them, not one band per row"
    );

    // ② A patch is three inks, and two of them stand on bands.
    assert_eq!(
        preview::preview_view("fix.diff", preview::preview_ftype("fix.diff"), false),
        preview::PreviewView::Diff
    );
    let patch = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n context\n-gone\n+here\n";
    let metrics = seats::preview_diff_metrics(scale);
    let margin = (seats::PREVIEW_DIFF_HUNK_MARGIN_LOGICAL_PX * scale).round();
    let mut top = 0.0_f32;
    let diff_rows: Vec<DiffRow> = patch
        .lines()
        .map(|line| {
            let kind = preview::diff_line_kind(line);
            if kind == preview::DiffLineKind::Hunk {
                top += margin;
            }
            let row = DiffRow {
                text: preview::expand_tabs(line),
                kind,
                top,
            };
            top += metrics.line_height;
            row
        })
        .collect();
    let diff = PreviewDocument::Diff(diff_rows.clone());
    let rows_height = diff_rows
        .last()
        .map_or(0.0, |row| row.top + metrics.line_height);
    let drawn = build_preview_diff_body(
        &seats::preview_mono_geometry(body, metrics, rows_height, 40, advance, [0.0, 0.0]),
        &diff_rows,
        &palette,
    );
    let inks: std::collections::BTreeSet<[u8; 3]> = drawn
        .paragraphs
        .iter()
        .flat_map(|line| line.runs.iter().map(|run| run.color))
        .collect();
    assert!(
        inks.len() >= 3,
        "an added line, a removed one and the context around them are three \
             different inks, saw {inks:?}"
    );
    assert_eq!(
        drawn.quads.len(),
        2,
        "and exactly the added and the removed lines stand on a band"
    );

    // ③ Both are sized by the same question the card asks, and the answer
    //    grows with the file.
    let height = |document: &PreviewDocument, rows_height| {
        preview_document_height(document, body, scale, advance, rows_height, 40)
    };
    let table_height = height(&table, 0.0);
    let diff_height = height(&diff, rows_height);
    assert!(
        table_height > 0.0 && diff_height > 0.0,
        "a document with content is a body with height"
    );
    let taller = PreviewDocument::Table {
        rows: rows.iter().cycle().take(rows.len() * 4).cloned().collect(),
        column_cells,
    };
    assert!(
        height(&taller, 0.0) > table_height,
        "and a longer table is a taller card"
    );
    assert_eq!(
        preview_document_height(&PreviewDocument::Empty, body, scale, advance, 0.0, 0),
        0.0,
        "a file whose head has not landed yet asks for no room at all"
    );
}

/// PIN (user ruling, 2026-08-14) — **the pane wears the card's own bar**.
///
/// The glance card got a scroll bar first, because it was the surface people
/// were visibly trying to read further into. That left the window upside
/// down: a 300-pixel hover card said how much of its file it was showing,
/// and the docked pane it opens into — the same document, four times the
/// size, scrolling under the same wheel — said nothing at all. A page you
/// can scroll and cannot see the extent of is a page you have to scroll to
/// find the end of.
///
/// So the three faces are asked the same four questions here, in one loop,
/// against one function. That is what "同原语" has to mean to be worth
/// anything: not three implementations that agree today, but one arithmetic
/// none of them can drift from — the lesson the block's bar already paid for
/// when its picture and its hit test disagreed.
///
/// MUTATIONS that must turn it red:
/// ① have `preview_body_bar` answer `None` for a box taller than it is wide
///    — which is exactly "a docked pane draws no bar": both pane shapes go
///    red and the 300×246 card stays green, which is the upside-down window
///    this ruling overturned;
/// ② hand `preview::scroll_bar` `ScrollAxis::Horizontal` — the rule lands
///    along the bottom edge and ① goes red for all three;
/// ③ read the offset from `scroll[0]` — ③ goes red, the thumb having stopped
///    following the wheel.
#[test]
fn a_docked_pane_and_a_float_wear_the_same_bar_the_glance_card_does() {
    const SCALE: f32 = 2.0;
    // A tall docked pane, a squat float, and the card — the three surfaces
    // the ruling names, in the three shapes they actually come in.
    let faces = [
        ("seat", [0.0, 80.0, 420.0, 900.0]),
        ("float", [300.0, 120.0, 760.0, 420.0]),
        (
            "card",
            [
                40.0,
                300.0,
                40.0 + file_peek::body_width(SCALE),
                300.0 + file_peek::body_max_height(SCALE, true),
            ],
        ),
    ];
    for (what, body) in faces {
        let page = body[3] - body[1];
        let content = page * 4.0;

        // ① Down the right edge of the body, never across it, and never past
        //    the bottom of the box the document is drawn in — the truncation
        //    bar and the foot stand below `body` and are not scrolled over.
        let bar = preview_body_bar(
            body,
            preview::ScrollAxis::Vertical,
            [0.0, 0.0],
            content,
            SCALE,
        )
        .unwrap_or_else(|| panic!("{what}: a document four pages long wears a bar"));
        assert_eq!(bar.axis, preview::ScrollAxis::Vertical, "{what}");
        assert_eq!([bar.track[1], bar.track[3]], [body[1], body[3]], "{what}");
        assert_eq!(bar.track[2], body[2], "{what}: the bar hugs the far edge");
        assert!(
            bar.track[0] > body[0] + (body[2] - body[0]) / 2.0,
            "{what}: a rule down the edge, not a curtain across the body"
        );

        // ② The overflow it offers is the overflow the clamp allows, so the
        //    end of the track and the end of the wheel are one place.
        assert_eq!(bar.overflow, content - page, "{what}");
        assert!(
            bar.thumb[3] - bar.thumb[1] >= preview::BLOCK_SCROLL_MIN_THUMB_LOGICAL_PX * SCALE,
            "{what}: a thumb no hand can see is a thumb no hand can take"
        );

        // ③ The thumb is a picture of the offset: at rest at the head of the
        //    track, at the end of the document at the end of it.
        assert_eq!(bar.thumb[1], body[1], "{what}: at rest, at the top");
        let end = preview_body_bar(
            body,
            preview::ScrollAxis::Vertical,
            [0.0, bar.overflow],
            content,
            SCALE,
        )
        .expect("still overflowing");
        assert!(
            (end.thumb[3] - body[3]).abs() < 0.5,
            "{what}: at the end of the file, at the end of the track"
        );

        // ④ And dragging reads that picture backwards, stopping where the
        //    wheel stops at both ends.
        let held = 5.0_f32;
        let dragged = |y: f32| preview::scroll_dragged_to(&bar, bar.along([body[2], y]), held);
        assert_eq!(dragged(bar.thumb[1] + held), 0.0, "{what}");
        assert_eq!(
            dragged(bar.thumb[1] + held + bar.travel * 9.0),
            bar.overflow,
            "{what}: past the end of the track is the end of the document"
        );

        // ⑤ **The hand's target reaches inward and stops at the edge**
        //    (real-machine finding, 2026-08-14). The two pixels of rule sit
        //    on the surface's own far edge, and everything past that edge
        //    has an owner that is asked first: a divider's seam band, or the
        //    window's resize border, which is answered in `WM_NCHITTEST`
        //    before this window sees a pointer at all. A bar that only
        //    reached five pixels each way was a bar drawn inside somebody
        //    else's band — visible, wheel-accurate, and impossible to take.
        assert!(
            bar.grab[0] <= bar.thumb[0] - preview::BODY_SCROLL_INWARD_HIT_LOGICAL_PX * SCALE,
            "{what}: the target has to clear the divider band and the resize border"
        );
        assert_eq!(
            bar.grab[2], body[2],
            "{what}: and it stops at the edge — the far side belongs to somebody else"
        );

        // ⑥ A document that fits wears nothing at all: a track with no thumb
        //    is a promise of somewhere to go in a pane that has nowhere.
        assert!(
            preview_body_bar(body, preview::ScrollAxis::Vertical, [0.0, 0.0], page, SCALE)
                .is_none(),
            "{what}: a file that fits its pane draws no bar"
        );
    }
}

/// PIN (user rulings, 2026-08-13) — **which markdown blocks insist on a
/// width, and which have no opinion.**
///
/// Three rulings, one arithmetic, and the arithmetic is the whole of the
/// horizontal scroll a rendered document gets:
///
/// ① a **code fence does not reflow** — code that wraps lies about its own
///    indentation, the same argument that keeps a `.diff` unwrapped — so it
///    answers its longest line and the pane scrolls to it;
/// ② a **table is as wide as its own columns**, and the pane scrolls to it
///    rather than compressing them: the first draft divided the pane's width
///    among the columns and wrapped inside the cells, and that policy was
///    overruled before it shipped;
/// ③ a **paragraph never insists**, however long the token inside it. A
///    180-character unbreakable run is broken mid-token by the shaper rather
///    than allowed to push a horizontal axis onto a page of prose — which is
///    what "soft wrap with an anywhere break" means and the reason a wall of
///    text still reads in a narrow pane.
///
/// Asserted on `stress.md`, the fixture built to carry all three at once.
///
/// MUTATION ①: answer `0.0` for the fence's width and the fence assertion
/// goes red — the long line is drawn and can never be reached.
/// MUTATION ②: go back to `width`-proportional columns and the table's own
/// width stops exceeding the pane, so the table assertion goes red.
/// MUTATION ③: give `Paragraph` a `width` and the prose assertion goes red,
/// which is a page of text growing a horizontal scrollbar.
#[test]
fn only_the_blocks_that_refuse_to_reflow_ask_for_a_horizontal_scroll() {
    let source = include_str!("../../../tests/assets/preview-samples/stress.md");
    let blocks = preview::parse_markdown(source);
    let fences = blocks
        .iter()
        .filter(|block| matches!(block, preview::MarkdownBlock::Code { .. }))
        .count();
    let tables: Vec<&preview::MarkdownBlock> = blocks
        .iter()
        .filter(|block| matches!(block, preview::MarkdownBlock::Table { .. }))
        .collect();
    assert!(fences >= 1, "the fixture carries a fence");
    assert_eq!(tables.len(), 2, "and a wide table and a narrow one");
    if let preview::MarkdownBlock::Table { rows, .. } = tables[0] {
        assert!(
            rows[0].len() >= 8,
            "the wide table is at least eight across"
        );
    }
    // The 180-character token really is one token, or ③ proves nothing.
    assert!(
        source
            .split_whitespace()
            .any(|token| token.chars().count() >= 180),
        "the fixture carries an unbreakable run long enough to matter"
    );

    // **The two insisting blocks, measured through the real arithmetic with
    // a stand-in shaper**: eight pixels a character, which is a monospace
    // grid and exactly what the assertions below can therefore predict.
    let metrics_at_1 = seats::preview_markdown_metrics(1.0);
    let cell_px = 8.0_f32;
    let fence_text = blocks
        .iter()
        .find_map(|block| match block {
            preview::MarkdownBlock::Code { text, .. } => Some(text.clone()),
            _ => None,
        })
        .expect("the fixture carries a fence");
    let longest = fence_text
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    assert!(longest >= 250, "the fence's longest line is a stress case");
    let fence_width = markdown_fence_width(&fence_text, metrics_at_1, |runs| {
        runs.iter()
            .map(|run| run.text.chars().count())
            .sum::<usize>() as f32
            * cell_px
    });
    assert_eq!(
        fence_width,
        longest as f32 * cell_px + (metrics_at_1.code_border + metrics_at_1.code_padding_x) * 2.0,
        "① the fence asks for its longest line and its own chrome"
    );

    let preview::MarkdownBlock::Table { rows, .. } = tables[0] else {
        unreachable!("filtered above")
    };
    let columns = markdown_table_columns(rows, metrics_at_1, |cell, _| {
        cell.iter()
            .map(|span| span.text.chars().count())
            .sum::<usize>() as f32
            * cell_px
    });
    assert_eq!(columns.len(), rows[0].len(), "one width per column");
    let chrome = metrics_at_1.table_border + metrics_at_1.table_padding_x * 2.0;
    for (index, width) in columns.iter().enumerate() {
        let widest = rows
            .iter()
            .filter_map(|row| row.get(index))
            .map(|cell| cell.iter().map(|s| s.text.chars().count()).sum::<usize>())
            .max()
            .unwrap_or(0) as f32
            * cell_px;
        assert_eq!(
            *width,
            chrome + widest.max(metrics_at_1.table_min_column),
            "② column {index} is its own widest cell, never a share of the pane"
        );
    }
    assert!(
        columns.iter().sum::<f32>() > 900.0,
        "and the table is genuinely wider than a pane, or this proves nothing"
    );

    // A layout is what the runtime would have measured, with the ruling's
    // shape and no renderer: the fence and the table insist, prose does not.
    let metrics = seats::preview_markdown_metrics(1.0);
    let pane = [0.0, 0.0, 300.0, 200.0];
    let prose_only = vec![
        MarkdownBlockLayout::solid(metrics.line_height * 12.0),
        MarkdownBlockLayout::solid(metrics.line_height * 3.0),
    ];
    let document = PreviewDocument::Markdown {
        intrinsic: Vec::new(),
        blocks: Vec::new(),
        ranges: Vec::new(),
        maps: Vec::new(),
        source: SourceBlocks::default(),
        layout: prose_only.into(),
        math: DocumentMath::default(),
        pictures: DocumentPictures::default(),
        wrap: Arc::default(),
    };
    let max = preview_document_max_scroll(&document, pane, 1.0, 8.0, 0.0, 0);
    assert_eq!(
        max[0], 0.0,
        "③ a page of prose has no horizontal axis, whatever it is made of"
    );

    // **And neither does a page holding a block that refuses to reflow**
    // (user ruling, 2026-08-13, overturning the same day's earlier reading).
    // The fence and the table still insist on their widths — the two
    // assertions above are about exactly that — but the width is spent
    // *inside the block*, not by sliding the whole page under the pane. The
    // page's own axis is zero either way, and this is the assertion that
    // says the ruling changed rather than the measurement.
    let wide = 900.0_f32;
    let with_a_fence = PreviewDocument::Markdown {
        intrinsic: Vec::new(),
        blocks: Vec::new(),
        ranges: Vec::new(),
        maps: Vec::new(),
        source: SourceBlocks::default(),
        layout: vec![
            MarkdownBlockLayout::solid(metrics.line_height),
            MarkdownBlockLayout {
                width: wide,
                ..MarkdownBlockLayout::solid(metrics.line_height * 4.0)
            },
        ]
        .into(),
        math: DocumentMath::default(),
        pictures: DocumentPictures::default(),
        wrap: Arc::default(),
    };
    let max = preview_document_max_scroll(&with_a_fence, pane, 1.0, 8.0, 0.0, 0);
    assert_eq!(
        max[0], 0.0,
        "①/② a wide block scrolls inside itself; the page it stands on does not move"
    );
}

/// PIN — **a sideways notch scrolls the table under the pointer, clamped at
/// both of its own ends, and a notch over prose is nobody's** (user ruling,
/// 2026-09-07, tier 3).
///
/// The report was that a tilt wheel did nothing at all: the axis was read off
/// `Shift` alone and the travel off `y` alone, so a report that was all `x`
/// came out zero at both ends of the arithmetic. This is the arithmetic that
/// answers it, with the pointer and the geometry handed in — the same numbers
/// [`preview_block_bar_at`] hands the thumb, so the wheel and the hand cannot
/// stop in different places.
///
/// MUTATIONS: drop the `clamp` in [`preview_block_wheel`] and a table can be
/// pushed past either end; hit-test on `at[0]` instead of `at[1]` and the
/// prose case starts taking notches that belong to the page.
#[test]
fn a_sideways_notch_scrolls_the_table_under_the_pointer_and_clamps_at_both_ends() {
    let metrics = seats::preview_markdown_metrics(1.0);
    // Wide enough that the column is capped, so the table has more width than
    // the column can hold and a scroll to do inside it.
    let body = [0.0, 0.0, 1771.0, 900.0];
    let blocks = preview::parse_markdown("Prose.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
    assert_eq!(blocks.len(), 2, "a paragraph and a table");
    assert!(matches!(blocks[1], preview::MarkdownBlock::Table { .. }));
    let wide = 3000.0;
    let table_top = metrics.line_height + metrics.paragraph_gap;
    let layout: preview_viewport::Layout = vec![
        MarkdownBlockLayout::solid(metrics.line_height),
        MarkdownBlockLayout {
            width: wide,
            top: table_top,
            ..MarkdownBlockLayout::solid(metrics.line_height * 3.0)
        },
    ]
    .into();
    let (left, right) = preview::markdown_measure_box(body, metrics);
    let overflow = wide - (right - left);
    assert!(overflow > 0.0, "a table this wide has to scroll");
    let origin = body[1] + metrics.padding_y;
    let on_the_table = [(left + right) / 2.0, origin + table_top + 2.0];
    let on_the_prose = [(left + right) / 2.0, origin + 1.0];
    let wheel = |offsets: &[f32], at: [f32; 2], travel: f32| {
        preview_block_wheel(
            body,
            metrics,
            [0.0, 0.0],
            offsets,
            (&blocks, &layout),
            at,
            travel,
        )
    };

    // A tilt to the right reaches this window as a negative `x`, and so as a
    // negative travel — `Runtime::wheel_travel`'s "positive goes back" — and
    // it moves the table forward.
    assert_eq!(wheel(&[0.0, 0.0], on_the_table, -40.0), Some((1, 40.0)));
    assert_eq!(
        wheel(&[0.0, 40.0], on_the_table, 40.0),
        Some((1, 0.0)),
        "and a tilt the other way brings it back"
    );

    // Clamped at both ends, by the numbers the thumb clamps by.
    assert_eq!(
        wheel(&[0.0, 0.0], on_the_table, 40.0),
        Some((1, 0.0)),
        "it stops at its own start"
    );
    assert_eq!(
        wheel(&[0.0, overflow], on_the_table, -400.0),
        Some((1, overflow)),
        "and at its own end, rather than handing the notch on to a page that \
             has nowhere to go either"
    );

    // A notch over prose is not this axis's: it goes on to the page, and a
    // markdown page has no horizontal axis, so nothing moves.
    assert_eq!(wheel(&[0.0, 0.0], on_the_prose, -40.0), None);
    // Nor is one outside the document's box at all.
    assert_eq!(
        wheel(&[0.0, 0.0], [body[2] + 10.0, on_the_table[1]], -40.0),
        None
    );
}

/// PIN — **a table is never wider than the prose column, at any pane width**
/// (user ruling, 2026-09-07, superseding the same day's earlier "a table may
/// bleed past it").
///
/// The bleed was built and photographed first: a table that needed more room
/// than the column reached up to fifteen per cent of it past the edge on each
/// side, bounded by the page's own padding. Shown the picture, the user's
/// verdict was that it is ugly — one block wider than everything else on the
/// page puts the eye on a ragged edge instead of on the document, and a page
/// whose blocks do not line up is not a page. So every block is set in the
/// same column the prose is, and a table that does not fit scrolls inside
/// itself, which is the whole of what the thumb along its foot is for.
///
/// Three widths, because "never wider" is a claim about all of them: a pane
/// narrower than the column has ever been, one between the old cap and the
/// new one, and one wide enough for the cap to bite.
///
/// MUTATION: give a table a box of its own — anything other than
/// `preview::markdown_measure_box` — and every width goes red.
#[test]
fn a_wide_table_is_drawn_into_the_prose_column_and_wears_a_thumb_along_its_own_foot() {
    const SCALE: f32 = 1.0;
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(SCALE);
    let blocks = preview::parse_markdown("| a | b |\n|---|---|\n| 1 | 2 |\n");
    assert_eq!(blocks.len(), 1, "a table and nothing else");
    let wide = 3000.0;
    let layout: preview_viewport::Layout = vec![MarkdownBlockLayout {
        width: wide,
        ..MarkdownBlockLayout::solid(metrics.line_height * 3.0)
    }]
    .into();
    let box_of = |body: [f32; 4]| {
        let built = markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(&[0.0]),
            (&blocks, &layout),
            &palette,
        );
        built
            .blocks
            .first()
            .expect("a table wider than the column scrolls inside itself")
            .clone()
    };

    // ① The box is the column's, at every width — not one pixel of it is
    //    the table's own.
    for body in [
        [0.0, 0.0, 500.0, 600.0],
        [0.0, 0.0, 900.0, 600.0],
        [0.0, 0.0, 1771.0, 900.0],
    ] {
        let block = box_of(body);
        assert_eq!(
            (block.clip[0], block.clip[2]),
            preview::markdown_measure_box(body, metrics),
            "a table stands in the prose column, in a pane {} wide",
            body[2] - body[0]
        );
    }

    // ② The thumb: flush to the box's own foot, the family's thickness, and
    //    as long as the share of the table the box is showing.
    let body = [0.0, 0.0, 1771.0, 900.0];
    let block = box_of(body);
    let (left, right) = preview::markdown_measure_box(body, metrics);
    let thumb = block.quads.last().expect("a bar has a thumb");
    let thickness = (preview::BLOCK_SCROLL_THICKNESS_LOGICAL_PX * SCALE)
        .round()
        .max(1.0);
    assert_eq!(
        thumb.rect[3], block.clip[3],
        "flush to the box's bottom edge"
    );
    assert_eq!(thumb.rect[3] - thumb.rect[1], thickness);
    assert_eq!(thumb.rect[0], block.clip[0], "at rest it starts at the box");
    let page = right - left;
    assert!(
        (thumb.rect[2] - thumb.rect[0] - page * (page / wide)).abs() < 0.01,
        "a thumb is the visible share of the content, drawn in proportion"
    );
}

/// RED GATE (user report 2026-09-14, the badge row; §7.1.3k ⑬) — **a chip
/// is drawn as a word and says the rest on a hover.**
///
/// The card prints two facts beside the alt text — why there is no picture,
/// and the address — and both of them are why it is three lines tall. A chip
/// has one line and it is sharing it, so the two facts move to the window's
/// own tip host, which is where a fact about a run belongs once the run is
/// the size of a word. Both of them, and in the card's own words: a second
/// wording would be this window explaining the same policy twice.
///
/// It is also the gate on the chip being *pressable*: a site with a target
/// is what makes the pointing finger, the underline and the press
/// ([`note_link_sites`]), and a chip that said its address on a card nobody
/// could act on would be worse than the card it replaced.
///
/// MUTATIONS: leave the sentence out of [`markdown_chip_tip`] and the chip
/// says an address with no reason beside it; let [`note_link_sites`] refuse
/// a chip as it refuses a picture and there is no site to hang either on.
#[test]
fn a_chips_hover_card_carries_the_sentence_and_the_address() {
    let blocks = preview::parse_markdown(
        "[![Build](https://img.example/build.svg)](https://ci.example) \
             [![Release](https://img.example/release.svg)](https://ci.example/releases)\n",
    );
    assert!(
        matches!(blocks.as_slice(), [preview::MarkdownBlock::Paragraph(_)]),
        "two badges on one line are one paragraph: {blocks:#?}",
    );
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
                metrics.line_height,
            )]),
            live: MarkdownLive::default(),
        },
        &bt_render::chrome_palette(),
        PageArt {
            math: &DocumentMath::default(),
            pictures: &DocumentPictures::default(),
            theme: bt_render::Theme::Dark,
        },
    );
    assert_eq!(
        card_text(&rendered),
        vec!["Build Release"],
        "the page is one line of two words and no card at all",
    );
    let chips: Vec<(&str, &str)> = rendered
        .links
        .iter()
        .filter_map(|site| Some((site.target.as_str(), site.chip.as_deref()?)))
        .collect();
    assert_eq!(
        chips.len(),
        2,
        "both badges answer a press and both carry a card: {:#?}",
        rendered.links,
    );
    let sentence = i18n::Text::MarkdownImageRemote.text();
    for (target, tip) in chips {
        assert!(
            tip.starts_with(sentence) && tip.contains(target),
            "the card says why, and then the address: {tip:?}",
        );
    }
}

/// **What a picture copies is `![alt](src)`** (§7.1.3k ④), and a selection
/// passes over it whole.
///
/// The display formula's answer one block kind over, for its reason: the alt
/// text alone would paste as a sentence the author never wrote, with nothing
/// to say that anything had been a picture.
#[test]
fn a_picture_copies_the_markdown_that_named_it() {
    let blocks = preview::parse_markdown("![a shot](docs/one.png)\n");
    let pieces = preview_select::pieces(&blocks);
    let [piece] = pieces.as_slice() else {
        panic!("one block, one piece: {pieces:#?}");
    };
    assert_eq!(piece.text, "![a shot](docs/one.png)");
    assert!(piece.atomic, "there is no offset into a picture");
    let all = preview_select::select_all(&pieces).expect("a document with a picture in it");
    let (start, end) = all.range(&pieces);
    assert_eq!(
        preview_select::copy_text(&pieces, start, end),
        "![a shot](docs/one.png)",
    );
}

/// PIN (same report) — **an inline formula becomes a gap the width of its
/// picture, and goes back to being text when there is none.**
///
/// The gap is the whole mechanism: prose reflows to the pane, so the only
/// honest way to put a picture inside a sentence is to make the sentence
/// leave room for it and then ask where the room ended up
/// ([`bt_render::PreviewRun::inline_box_px`]).
///
/// The delimiters in the fallback are not decoration. They are what tells a
/// reader that the letters beside them are a formula this window could not
/// set, rather than a typo in the prose — and they are the author's own
/// bytes, which is the standard this whole lane is held to.
///
/// MUTATIONS: emit the source text alongside the box and the first
/// assertion's `text` goes red; strip the dollars in the fallback and the
/// second does.
#[test]
fn an_inline_formula_is_a_gap_the_width_of_its_picture_or_the_text_it_was() {
    let palette = bt_render::chrome_palette();
    let spans = vec![
        preview::Span::plain("energy "),
        preview::Span::math("$E = mc^2$"),
        preview::Span::plain(" rules"),
    ];
    let metrics = seats::preview_markdown_metrics(1.0);
    let math = one_picture(
        "E = mc^2",
        MathMode::Inline,
        metrics.font_size,
        (57, 18, 13.0),
    );
    let set = markdown_runs(&spans, &palette, false, &math, metrics.font_size);
    assert_eq!(
        set.iter()
            .map(|run| (run.text.as_str(), run.inline_box_px))
            .collect::<Vec<_>>(),
        vec![("energy ", None), ("", Some(57.0)), (" rules", None),],
        "the formula is a box of its picture's width and no text at all",
    );
    let waiting = markdown_runs(
        &spans,
        &palette,
        false,
        &DocumentMath::default(),
        metrics.font_size,
    );
    assert_eq!(
        waiting
            .iter()
            .map(|run| (run.text.as_str(), run.inline_box_px))
            .collect::<Vec<_>>(),
        vec![("energy ", None), ("$E = mc^2$", None), (" rules", None),],
        "and with no picture it is the delimited source, exactly as written",
    );
    // **And a picture set for one size is not a picture for another.** A
    // heading's formula is asked for at the heading's size, so the body-size
    // entry above must not answer for it — otherwise a `# ` would draw a
    // formula a third the height of the letters beside it.
    let in_a_masthead = markdown_runs(&spans, &palette, true, &math, metrics.heading_font(1));
    assert_eq!(
        in_a_masthead
            .iter()
            .map(|run| (run.text.as_str(), run.inline_box_px))
            .collect::<Vec<_>>(),
        vec![("energy ", None), ("$E = mc^2$", None), (" rules", None),],
        "the body-size picture does not answer for a heading",
    );
}

/// PIN (user report, 2026-08-26: *md 的 hover 预览卡不显示 LaTeX 公式*) —
/// **the glance card asks the engine the pane's own question**, so a file
/// already set in a preview pane costs the card nothing at all.
///
/// The card is a 298-pixel mirror of a pane and the pane can be a thousand
/// wide: the two lay the same file out in boxes that do not resemble each
/// other, and the first assertion here is that they really do — a fixture
/// whose two layouts came out the same height would prove nothing about the
/// second.
///
/// The second is the ruling. What decides a formula's picture is its source,
/// its mode, its size and its ink ([`PreviewMathKey`]) — the measure it stands
/// in is not among them, because a picture is the same picture wherever the
/// prose around it broke. So the card's walk finds every formula the pane
/// already asked for **in flight**, and sends nothing.
///
/// MUTATION: give [`PreviewMathKey`] a surface, a path or a measure — the
/// obvious way to "give the card its own formulas" — and every one of them
/// comes back a miss, which on the glass is the same document typeset twice
/// and a second copy of every picture held in a budget sized for one.
#[test]
fn the_card_and_the_pane_ask_the_engine_for_one_and_the_same_picture() {
    let scale = 1.0;
    let metrics = seats::preview_markdown_metrics(scale);
    let blocks = preview::parse_markdown(
        "# The $\\alpha$ chapter\n\
             \n\
             A paragraph long enough that a three-hundred-pixel card and a pane \
             twice as wide cannot possibly wrap it to the same number of lines.\n\
             \n\
             $$E = mc^2$$\n",
    );
    // A shaper's answer in the one way a test can own: a line's worth of
    // room per seven pixels of text, folded to the width it was given.
    let mut wrap = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        let ink = runs
            .iter()
            .map(|run| run.text.chars().count() as f32 * 7.0)
            .sum::<f32>();
        line * (ink / width.max(1.0)).ceil().max(1.0)
    };
    let intrinsic = vec![MarkdownBlockIntrinsic::default(); blocks.len()];
    let height = |box_width: f32, wrap: &mut WrapMeasure<'_>| {
        let (left, right) = preview::markdown_measure_box([0.0, 0.0, box_width, 4000.0], metrics);
        lay_markdown_out(
            &blocks,
            &intrinsic,
            &NO_SOURCE_BLOCKS,
            right - left,
            metrics,
            PageArt {
                math: &DocumentMath::default(),
                pictures: &DocumentPictures::default(),
                theme: bt_render::Theme::Dark,
            },
            wrap,
        )
        .last()
        .map_or(0.0, |last| last.top + last.height)
    };
    let in_the_card = height(file_peek::body_width(scale), &mut wrap);
    let in_a_pane = height(900.0, &mut wrap);
    assert!(
        in_the_card > in_a_pane,
        "the two boxes are genuinely different measures: {in_the_card} vs {in_a_pane}",
    );

    // What each of them would ask the engine for, built exactly as
    // `resolve_document_math` builds it.
    let ink = bt_render::chrome_palette().files_row_text;
    let asked = || {
        document_formulas(&blocks, metrics)
            .into_iter()
            .map(|(source, mode, em_px)| PreviewMathKey {
                source,
                mode,
                em_milli_px: math_em_milli(em_px),
                foreground_rgb: ink,
            })
            .collect::<Vec<_>>()
    };
    let mut cache = PreviewMathCache::default();
    let from_the_pane = asked();
    assert_eq!(
        from_the_pane.len(),
        2,
        "the masthead's formula and the display block, or this proves nothing",
    );
    for key in from_the_pane.clone() {
        cache.mark_pending(key);
    }
    let missed = asked()
        .into_iter()
        .filter(|key| cache.get(key).is_none())
        .collect::<Vec<_>>();
    assert!(
        missed.is_empty(),
        "the card finds every one of them already in flight: {missed:?}",
    );
    // And the identity the GPU knows them by is the same identity, or the
    // one cache above would still be two textures.
    assert_eq!(
        asked()
            .iter()
            .map(PreviewMathKey::texture_key)
            .collect::<Vec<_>>(),
        from_the_pane
            .iter()
            .map(PreviewMathKey::texture_key)
            .collect::<Vec<_>>(),
    );
}

/// The document hands out every link it drew, addressed by the run it drew
/// it as (user ruling, 2026-08-13).
///
/// The address is the whole of this: a link is a *run inside a paragraph*,
/// and where that run landed is a question only the shaper answers
/// ([`bt_render::Renderer::measure_preview_run_boxes`]). What the painter
/// owes is the subscript, and the one that is easy to get wrong is a list
/// item's — its bullet rides in front of the text as a run of its own, so
/// the first span of an item is the paragraph's *second* run. Off by one
/// there and the pointer underlines the bullet and opens nothing.
///
/// MUTATIONS: pass `0` as a list item's first run and the second assertion
/// goes red; note the sites after pushing the paragraph rather than before,
/// and every subscript is one too far.
#[test]
fn a_document_hands_out_every_link_it_drew_with_the_run_that_drew_it() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 400.0, 400.0];
    let blocks = [
        preview::MarkdownBlock::Paragraph(vec![
            preview::Span::plain("see "),
            preview::Span::link("the design", "../../docs/DESIGN.md"),
            preview::Span::plain(" first"),
        ]),
        preview::MarkdownBlock::List {
            ordered: None,
            items: vec![vec![preview::Span::link("a note", "notes.md")]],
        },
    ];
    let layout: preview_viewport::Layout = [
        MarkdownBlockLayout::solid(metrics.line_height),
        MarkdownBlockLayout {
            top: metrics.line_height + metrics.paragraph_gap,
            ..MarkdownBlockLayout::rows(vec![metrics.line_height], 0.0)
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
    let sites: Vec<_> = rendered
        .links
        .iter()
        .map(|site| (site.block, site.paragraph, site.run, site.target.as_str()))
        .collect();
    assert_eq!(
        sites,
        vec![
            (None, 0, 1, "../../docs/DESIGN.md"),
            (None, 1, 1, "notes.md"),
        ],
        "the prose link is its paragraph's second run, and the item's link \
             is the item's second run because the bullet is its first"
    );
    // And the subscripts address the runs that were actually built.
    for (block, paragraph, run, target) in sites {
        assert!(block.is_none());
        let drawn = &rendered.body.paragraphs[paragraph].runs[run];
        assert_eq!(drawn.color, palette.accent, "a link is set in the accent");
        assert!(
            !drawn.text.contains(target),
            "and never prints where it points"
        );
    }
}

/// **The document hands out every piece it drew, addressed in the
/// document's own terms** (user report 2026-08-28: 「渲染后的 md 文字无法
/// 选中」).
///
/// The anti-drift pin for the whole feature. A selection is anchored to
/// `(block, piece, offset)` and the painter numbers those pieces by walking
/// the blocks; [`preview_select::pieces`] numbers them a second time, by
/// walking the same blocks, because building every string of a thousand-block
/// document on every frame to hand back indices the painter is already
/// counting would be paying a great deal for an agreement a test can hold.
/// So this is the test that holds it.
///
/// MUTATIONS: number a table's cells by the *column count* rather than by
/// the row's own length, and a short row puts every cell after it against
/// the wrong text; note a list item's site with `first_run` at zero and its
/// bullet becomes selectable text.
#[test]
fn a_document_hands_out_every_piece_it_drew_in_the_documents_own_terms() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 600.0, 900.0];
    let cell = |text: &str| vec![preview::Span::plain(text)];
    let blocks = [
        preview::MarkdownBlock::Heading {
            level: 2,
            spans: vec![preview::Span::plain("Title")],
        },
        preview::MarkdownBlock::Paragraph(vec![
            preview::Span::plain("see "),
            preview::Span::bold("this"),
        ]),
        preview::MarkdownBlock::List {
            ordered: None,
            items: vec![cell("one"), cell("two")],
        },
        preview::MarkdownBlock::Quote(vec![cell("quoted")]),
        preview::MarkdownBlock::Rule,
        preview::MarkdownBlock::Table {
            rows: vec![
                vec![cell("name"), cell("size")],
                vec![cell("a.txt"), cell("12")],
            ],
            alignments: vec![bt_detect::table::ColumnAlignment::None; 2],
        },
        preview::MarkdownBlock::Code {
            lang: Some("rust".to_owned()),
            text: "fn main() {}\nlet x = 1;".to_owned(),
        },
    ];
    let row = metrics.line_height;
    let layout: preview_viewport::Layout = vec![
        MarkdownBlockLayout {
            top: 0.0,
            ..MarkdownBlockLayout::solid(row)
        },
        MarkdownBlockLayout {
            top: row,
            ..MarkdownBlockLayout::solid(row)
        },
        MarkdownBlockLayout {
            top: 2.0 * row,
            ..MarkdownBlockLayout::rows(vec![row, row], 0.0)
        },
        MarkdownBlockLayout {
            top: 4.0 * row,
            ..MarkdownBlockLayout::rows(vec![row], 2.0 * metrics.quote_padding_y)
        },
        MarkdownBlockLayout {
            top: 6.0 * row,
            ..MarkdownBlockLayout::solid(metrics.rule_thickness)
        },
        MarkdownBlockLayout {
            top: 7.0 * row,
            columns: vec![120.0, 120.0],
            width: 240.0,
            ..MarkdownBlockLayout::rows(vec![row, row], metrics.table_border)
        },
        MarkdownBlockLayout {
            top: 10.0 * row,
            ..MarkdownBlockLayout::solid(
                2.0 * metrics.code_border
                    + 2.0 * metrics.code_padding_y
                    + 2.0 * metrics.code_line_height,
            )
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
    let pieces = preview_select::pieces(&blocks);
    assert_eq!(
        rendered
            .text
            .iter()
            .map(|site| site.piece.at)
            .collect::<Vec<_>>(),
        pieces.iter().map(|piece| piece.at).collect::<Vec<_>>(),
        "every piece of the document is set once, in the order it is read in",
    );
    for site in &rendered.text {
        let piece =
            preview_select::piece_at(&pieces, site.piece.at).expect("a piece of the document");
        assert_eq!(
            site.piece.len,
            piece.text.len(),
            "the painter and the model agree how long {:?} is",
            site.piece.at,
        );
        let PreviewTextWhere::Paragraph(at) = site.what else {
            panic!("nothing in this document is a picture");
        };
        let paragraph = match site.block {
            None => &rendered.body.paragraphs[at],
            Some(block) => &rendered.body.blocks[block].paragraphs[at],
        };
        let shaped = bt_render::preview_paragraph_text(paragraph);
        assert_eq!(
            &shaped[site.piece.lead..],
            piece.text,
            "and the glass carries exactly the bytes the model says it does",
        );
    }
    // The bullet is in front of the item's own text and is none of it: a
    // pointer cannot stand in a mark the document did not write.
    let item = rendered
        .text
        .iter()
        .find(|site| site.piece.at == preview_select::Place::new(2, 0, 0))
        .expect("the first list item");
    assert!(
        item.piece.lead > 0,
        "a list item's marker is lead the selection begins after",
    );
}

/// **A file with nothing in it can be typed into** (user report,
/// 2026-09-11: `New file…` made `happy.md`, and clicking in it did nothing
/// at all).
///
/// A page with no blocks has no piece of a parse to press on: there is no
/// box, no provenance and no source block, so every press named nothing and
/// was read as a press on the page's empty ground — which *leaves* a page
/// rather than entering it, and leaves the keyboard somewhere else. A file
/// with no bytes has exactly one place a caret can be.
///
/// MUTATION: answer for a page that has blocks and a press on the margin
/// beside a paragraph stops rendering the page again, which is the ruling
/// the empty-ground arm exists for.
#[test]
fn a_press_anywhere_in_an_empty_page_names_its_only_byte() {
    let body = [100.0, 40.0, 500.0, 400.0];
    let page = |content: &str| {
        let (blocks, ranges, maps) = preview::parse_markdown_mapped(content);
        PreviewDocument::Markdown {
            blocks,
            ranges,
            maps,
            source: SourceBlocks::default(),
            intrinsic: Vec::new(),
            layout: preview_viewport::Layout::default(),
            math: DocumentMath::default(),
            pictures: DocumentPictures::default(),
            wrap: Arc::default(),
        }
    };
    let empty = page("");
    assert_eq!(
        markdown_empty_page_offset(&empty, body, 100.0, 40.0),
        Some(0),
        "the body's own corner",
    );
    assert_eq!(
        markdown_empty_page_offset(&empty, body, 400.0, 300.0),
        Some(0),
        "and the middle of the ground under it, which is where a reader \
             actually presses",
    );
    assert_eq!(
        markdown_empty_page_offset(&empty, body, 99.0, 300.0),
        None,
        "outside the body is the furniture's, and the furniture is answered \
             above this in the press ladder",
    );
    assert_eq!(
        markdown_empty_page_offset(&empty, body, 400.0, 39.0),
        None,
        "the head stands over the document and is not it",
    );
    assert_eq!(
        markdown_empty_page_offset(&page("words\n"), body, 400.0, 300.0),
        None,
        "a page with words on it answers through its pieces, and a press on \
             its ground is still a press on its ground",
    );
}

/// **Typing into an empty file makes its first block** (user report,
/// 2026-09-11), through the ordinary re-parse and nothing else.
///
/// The second half is the end of a file that has no last break (audit A6):
/// the caret after the only character typed is at `content.len()`, which is
/// the last block's own `range.end` — read as a gap, the paragraph would
/// never be drawn as source and the bar would stand on a line below it that
/// does not exist.
#[test]
fn typing_into_an_empty_file_makes_the_first_block() {
    let mut content = String::new();
    let (_, ranges) = preview::parse_markdown_ranged(&content);
    assert!(ranges.is_empty(), "nothing was parsed out of nothing");
    let mut caret = preview_edit::EditCaret::default();
    assert_eq!(
        preview_live::caret_seat(&content, &ranges, caret.caret),
        preview_live::CaretSeat::Gap { after: None },
        "which is one empty source line at the top of the page",
    );
    assert!(preview_edit::insert(&mut content, &mut caret, "h"));
    assert_eq!(content, "h");
    assert_eq!(caret.caret, 1, "the caret is after what was typed");
    let (blocks, ranges) = preview::parse_markdown_ranged(&content);
    assert_eq!(blocks.len(), 1, "one paragraph, made by the parse");
    assert_eq!(
        preview_live::caret_seat(&content, &ranges, caret.caret),
        preview_live::CaretSeat::Block(0),
        "and the caret is inside it, so it is drawn as source",
    );
    assert_eq!(
        preview_live::place_in_block(&content, &ranges, 0, caret.caret),
        Some((0, 1)),
        "on its first line, one column in, which is where the bar is drawn",
    );
}

/// **A press on rendered text lands on the file byte that letter was copied
/// from** (T5 ①), which is the whole of what T6 was built for: a heading is
/// drawn without its hashes and a click on its first letter must not land on
/// the `#`.
///
/// MUTATION: answer with the block's `range.start` instead of asking the
/// piece, and every click in a paragraph lands at the top of it — the
/// gesture the research said would have to be taken away again.
#[test]
fn a_press_on_rendered_text_names_the_file_byte_it_draws() {
    let content = "# Title\n\nplain words here\n";
    let (blocks, ranges, maps) = preview::parse_markdown_mapped(content);
    assert_eq!(blocks.len(), 2, "a heading and a paragraph");
    let at = |block, piece, offset| {
        preview_provenance::file_offset_of(
            &preview_select::Place::new(block, piece, offset),
            &blocks,
            &ranges,
            &maps,
        )
    };
    assert_eq!(
        at(0, 0, 0),
        Some(2),
        "the first letter of the heading is the byte after `# `",
    );
    assert_eq!(at(1, 0, 6), Some(15), "the seventh letter of the paragraph");
    assert_eq!(
        at(1, 0, 16),
        Some(25),
        "and a click past the last letter is one past the last byte copied, \
             which is where a caret at the end of a paragraph stands",
    );
}

/// **A press in the tissue between two blocks is the nearer block's** (T5's
/// gap arm).
///
/// Nothing is ever parsed out of a blank line (§7.1.3o), so there is no
/// piece under the pointer out here; the page answers with the piece it is
/// nearest to in document order, and that piece then names a byte of the
/// file exactly as one under the pointer would.
#[test]
fn a_press_in_the_gap_between_two_blocks_is_answered_by_the_nearer_one() {
    let content = "# Title\n\nplain words here\n";
    let (blocks, ranges, maps) = preview::parse_markdown_mapped(content);
    let boxes = vec![
        row_box(preview_select::Place::new(0, 0, 0), 5, "Title", 0.0, 20.0),
        row_box(
            preview_select::Place::new(1, 0, 0),
            16,
            "plain words here",
            60.0,
            80.0,
        ),
    ];
    let nearer = |y: f32| {
        let at = preview_text_box_at(&boxes, 10.0, y).expect("a page with text answers");
        preview_provenance::file_offset_of(&boxes[at].piece.at, &blocks, &ranges, &maps)
    };
    assert_eq!(nearer(30.0), Some(2), "nearer the heading above it");
    assert_eq!(nearer(55.0), Some(9), "nearer the paragraph below it");
    assert_eq!(
        nearer(900.0),
        Some(9),
        "and past the end of the document is its last block",
    );
}

/// **Typing moves the file and the source block follows it** (T5 ②).
///
/// The parse is the whole mechanism: nothing tells a block it has been typed
/// into, the bytes move, the document is parsed again, and the block whose
/// range holds the caret is the one drawn as source.
#[test]
fn typing_moves_the_file_and_the_source_block_follows() {
    let mut content = String::from("# head\n\nbody\n");
    let (_, ranges) = preview::parse_markdown_ranged(&content);
    let mut caret = preview_edit::EditCaret {
        anchor: 12,
        caret: 12,
        desired_column: None,
        desired_x: None,
    };
    assert_eq!(
        preview_live::caret_seat(&content, &ranges, caret.caret),
        preview_live::CaretSeat::Block(1),
    );
    assert!(preview_edit::insert(&mut content, &mut caret, "!"));
    assert_eq!(content, "# head\n\nbody!\n");
    let (_, ranges) = preview::parse_markdown_ranged(&content);
    assert_eq!(
        preview_live::caret_seat(&content, &ranges, caret.caret),
        preview_live::CaretSeat::Block(1),
        "still the paragraph, one byte longer",
    );
    assert_eq!(preview_live::block_source(&content, &ranges[1]), "body!");
}

/// **A caret selection is drawn on both faces at once** (T5 ④): the source
/// block cuts the file range against itself, and every other block is found
/// by mapping the same two file bytes back onto the page.
///
/// MUTATION: map only the start and reuse it for the end and a selection
/// that leaves its block draws a band of nothing.
#[test]
fn a_caret_selection_spanning_two_blocks_maps_onto_both() {
    let content = "# Title\n\nplain words here\n";
    let (blocks, ranges, maps) = preview::parse_markdown_mapped(content);
    let start = preview_provenance::place_of(2, &blocks, &ranges, &maps)
        .expect("a page with words answers");
    let end = preview_provenance::place_of(15, &blocks, &ranges, &maps)
        .expect("a page with words answers");
    assert_eq!(
        (start.block, start.offset),
        (0, 0),
        "the byte after the hashes is the heading's first drawn letter",
    );
    assert_eq!(
        (end.block, end.offset),
        (1, 6),
        "and the far end is six letters into the paragraph under it",
    );
}

/// **The refusals are the buffer's and they stay honest** (T5 ⑥).
///
/// Turning the rendered face on turned it on for the *name and the type*
/// (`preview::is_editable`); everything a buffer can be wrong about is still
/// asked separately, and each of these is a page that draws, reads and
/// copies exactly as before and takes no caret at all.
#[test]
fn a_page_this_window_will_not_edit_takes_no_caret_on_either_face() {
    let mut whole = text_buffer("notes.md", "# head\n\nbody\n");
    assert!(!whole.awaits_the_whole_file());
    assert!(!whole.ask_for_the_whole_file(false));
    assert!(
        whole.is_editable(false) && whole.is_editable(true),
        "the ordinary Markdown file, both faces",
    );

    let mut truncated = preview::PreviewBuffer::new(
        preview::PreviewSource::file(r"C:\w\long.md"),
        "long.md".to_owned(),
    );
    truncated.accept(preview::HeadOutcome::Read {
        text: "# head\n".to_owned(),
        truncated: true,
        mtime: None,
        content_says_text: true,
        encoding: preview::HeadEncoding::Utf8,
        lossy: false,
    });
    assert!(
        !truncated.is_editable(false),
        "genuinely incomplete content remains read-only during whole-load recovery",
    );

    assert!(truncated.awaits_the_whole_file());
    let mut lossy = preview::PreviewBuffer::new(
        preview::PreviewSource::file(r"C:\w\odd.md"),
        "odd.md".to_owned(),
    );
    lossy.accept(preview::HeadOutcome::Read {
        text: "# head\u{fffd}\n".to_owned(),
        truncated: false,
        mtime: None,
        content_says_text: true,
        encoding: preview::HeadEncoding::Utf8,
        lossy: true,
    });
    assert!(
        !lossy.is_editable(false),
        "and bytes this window invented a stand-in for are never written back",
    );

    assert!(!lossy.awaits_the_whole_file());
    let table = text_buffer("cases.csv", "a,b\n1,2\n");
    assert!(!table.is_editable(false) && !table.is_editable(true));
    let diff = text_buffer("change.diff", "--- a\n+++ b\n");
    assert!(!diff.is_editable(false) && !diff.is_editable(true));
}

/// PIN (user report, 2026-08-13: "拖窗口边时 md 预览明显卡") — **a resize
/// re-flows and does not re-measure.**
///
/// Asserted by counting the shaper's calls, which is the honest unit: the
/// measurement *is* the cost, and a wall clock would be measuring this
/// machine. Against `docs/UI-UX.md` — the document the report was made with —
/// at every width a drag passes through.
///
/// The two halves the fix rests on:
///
/// ① the parse key does not contain the width, so a resize does not re-parse
///    the file at all;
/// ② the width-independent measurements — a table's columns, a fence's
///    longest line — are not asked again, so the per-width pass touches only
///    the blocks that genuinely reflow. On this document that is the
///    difference between thousands of shaping calls per pixel of drag and
///    hundreds.
///
/// MUTATIONS:
/// ① put `body_width_px` back into `PreviewParseKey` — the first assertion
///    goes red, and the whole pipeline runs per pixel again;
/// ② have the `Table`/`Code` arms of `measure_markdown_block` re-derive their
///    widths instead of reading the intrinsic — the second assertion goes
///    red, and it is the one that dominates the profile.
#[test]
fn a_resize_reflows_the_markdown_without_re_measuring_it() {
    let source = include_str!("../../../docs/UI-UX.md");
    let blocks = preview::parse_markdown(source);
    let metrics = seats::preview_markdown_metrics(1.0);
    let cell = 8.0_f32;

    // ① The parse survives every width; only the layout key moves.
    let mut buffer = preview::PreviewBuffer::new(
        preview::PreviewSource::file(r"C:\w\UI-UX.md"),
        "UI-UX.md".to_owned(),
    );
    buffer.accept(preview::HeadOutcome::Read {
        text: source.to_owned(),
        truncated: false,
        mtime: None,
        content_says_text: true,
        encoding: preview::HeadEncoding::Utf8,
        lossy: false,
    });
    let wide = document_key(&buffer, false, 1200.0, 1.0);
    let narrow = document_key(&buffer, false, 400.0, 1.0);
    assert_ne!(wide, narrow, "a width change is still a layout change");
    assert_eq!(
        wide.parse, narrow.parse,
        "① but not a parse change: the width is not in the parse key"
    );
    // And an edit is, so the cache cannot go stale.
    buffer.edit_content(|content| {
        content.insert(0, 'x');
        true
    });
    assert_ne!(
        document_key(&buffer, false, 1200.0, 1.0).parse,
        wide.parse,
        "an edit re-parses"
    );

    // ② The intrinsics are measured once; the per-width pass never asks about
    //    a table cell or a fence line again.
    let intrinsic_calls = std::cell::Cell::new(0usize);
    let intrinsic: Vec<MarkdownBlockIntrinsic> = blocks
        .iter()
        .map(|block| match block {
            preview::MarkdownBlock::Table { rows, .. } => {
                let columns = markdown_table_columns(rows, metrics, |cell_spans, _| {
                    intrinsic_calls.set(intrinsic_calls.get() + 1);
                    cell_spans
                        .iter()
                        .map(|span| span.text.chars().count())
                        .sum::<usize>() as f32
                        * cell
                });
                MarkdownBlockIntrinsic {
                    width: columns.iter().sum::<f32>() + metrics.table_border,
                    columns,
                    rows: rows.len(),
                    ..MarkdownBlockIntrinsic::default()
                }
            }
            preview::MarkdownBlock::Code { text, .. } => MarkdownBlockIntrinsic {
                width: markdown_fence_width(text, metrics, |runs| {
                    intrinsic_calls.set(intrinsic_calls.get() + 1);
                    runs.iter()
                        .map(|run| run.text.chars().count())
                        .sum::<usize>() as f32
                        * cell
                }),
                rows: text.lines().count().max(1),
                ..MarkdownBlockIntrinsic::default()
            },
            _ => MarkdownBlockIntrinsic::default(),
        })
        .collect();
    assert!(
        intrinsic_calls.get() > 100,
        "the fixture's tables and fences really are a measurable share              ({} shaping calls), or this proves nothing",
        intrinsic_calls.get()
    );

    let reflow_calls = std::cell::Cell::new(0usize);
    let mut measure = |runs: &[bt_render::PreviewRun], width: f32, _: f32, line: f32| {
        reflow_calls.set(reflow_calls.get() + 1);
        let columns = runs
            .iter()
            .map(|run| run.text.chars().count())
            .sum::<usize>() as f32
            * cell;
        (columns / width.max(1.0)).ceil().max(1.0) * line
    };

    // **The sharp statement**: a document made of nothing but the two blocks
    // whose geometry is intrinsic re-flows without asking the shaper a single
    // question. That is the whole of what a drag stops paying for.
    let heavy: Vec<preview::MarkdownBlock> = blocks
        .iter()
        .filter(|block| {
            matches!(
                block,
                preview::MarkdownBlock::Table { .. } | preview::MarkdownBlock::Code { .. }
            )
        })
        .cloned()
        .collect();
    let heavy_intrinsic: Vec<MarkdownBlockIntrinsic> = blocks
        .iter()
        .zip(&intrinsic)
        .filter(|(block, _)| {
            matches!(
                block,
                preview::MarkdownBlock::Table { .. } | preview::MarkdownBlock::Code { .. }
            )
        })
        .map(|(_, intrinsic)| intrinsic.clone())
        .collect();
    assert!(
        heavy.len() > 5,
        "the fixture carries enough of them to matter"
    );
    let before = reflow_calls.get();
    let heavy_layout = lay_markdown_out(
        &heavy,
        &heavy_intrinsic,
        &NO_SOURCE_BLOCKS,
        400.0,
        metrics,
        PageArt {
            math: &DocumentMath::default(),
            pictures: &DocumentPictures::default(),
            theme: bt_render::Theme::Dark,
        },
        &mut measure,
    );
    assert_eq!(
        reflow_calls.get(),
        before,
        "② a table and a fence are laid out from what was already measured"
    );
    assert!(
        heavy_layout.iter().all(|block| block.height > 0.0),
        "and they are laid out, not skipped"
    );

    // And the pass that remains is a real re-flow: a narrower pane is a
    // taller page, at a cost that does not grow with the document's tables.
    let mut heights = Vec::new();
    for width in [1200.0, 900.0, 700.0, 500.0, 380.0] {
        let layout = lay_markdown_out(
            &blocks,
            &intrinsic,
            &NO_SOURCE_BLOCKS,
            width,
            metrics,
            PageArt {
                math: &DocumentMath::default(),
                pictures: &DocumentPictures::default(),
                theme: bt_render::Theme::Dark,
            },
            &mut measure,
        );
        heights.push(layout.last().map_or(0.0, |last| last.top + last.height));
    }
    for pair in heights.windows(2) {
        assert!(
            pair[1] > pair[0],
            "a narrower pane wraps to more lines: {pair:?}"
        );
    }
}

/// **The caret crossing a block boundary re-lays-out and does not
/// re-parse** (§7.1.3q).
///
/// Asserted through the key, which is this pipeline's own seam for it and
/// the very one `a_resize_reflows_the_markdown_without_re_measuring_it` uses
/// one clause up: `rebuild_preview_document` re-parses exactly when the
/// *parse* half of the key moves, so "the same content re-lays-out" is
/// `key != key` with `key.parse == key.parse`, in as many words.
///
/// MUTATION ①: leave the source block out of `PreviewDocumentKey` and the
/// first assertion goes red — the caret leaves a block and the page goes on
/// drawing it as source, because nothing in the key moved.
/// MUTATION ②: put it in `PreviewParseKey` instead and the second goes red —
/// every arrow key across a paragraph re-parses and re-highlights the whole
/// document, which is the 2026-08-13 resize stutter with a keyboard in front
/// of it.
/// MUTATION ③: key on the caret's offset rather than on the block and the
/// third goes red — a caret walking along one paragraph re-lays-out the page
/// per keypress for a body that is identical.
#[test]
fn the_caret_changing_block_is_a_layout_change_and_not_a_parse_change() {
    let source = "# head\n\nfirst paragraph\n\nsecond paragraph\n";
    let mut buffer = preview::PreviewBuffer::new(
        preview::PreviewSource::file(r"C:\w\live.md"),
        "live.md".to_owned(),
    );
    buffer.accept(preview::HeadOutcome::Read {
        text: source.to_owned(),
        truncated: false,
        mtime: None,
        content_says_text: true,
        encoding: preview::HeadEncoding::Utf8,
        lossy: false,
    });
    let (blocks, ranges) = preview::parse_markdown_ranged(source);
    let key = |caret: Option<usize>| {
        let seat = caret.and_then(|caret| {
            preview_live::source_span(source, &ranges, &blocks, caret..caret, caret)
        });
        preview_document_key(
            &buffer,
            false,
            1200.0,
            1.0,
            PageArtKey {
                math_generation: 0,
                body_ink: [0, 0, 0],
                picture_generation: 0,
                picture_reach: PictureReach::from_the_top(),
                theme: bt_render::Theme::Dark,
            },
            seat,
        )
    };
    let heading = key(Some(1));
    let paragraph = key(Some(10));
    assert_ne!(
        heading, paragraph,
        "① a different block is drawn as source, so the page is laid out again",
    );
    assert_eq!(
        heading.parse, paragraph.parse,
        "② and not parsed again: the caret moved, the bytes did not",
    );
    assert_eq!(
        paragraph,
        key(Some(14)),
        "③ a caret walking along one block is not a layout change at all",
    );
    assert_ne!(
        paragraph,
        key(Some(8 + ranges[1].len())),
        "and stepping off the end of it into the blank line is one, because \
             the blank line belongs to no block",
    );
    assert_ne!(
        heading,
        key(None),
        "a page with no caret has no source block"
    );
}

/// RED (owner's ruling 2026-09-23) — **a selection that reaches into another
/// block is a layout change, and one that stays inside its block is not.**
///
/// The key carries the source span, so the page is laid out again exactly when
/// the set of blocks drawn as source moves — never per character a
/// Shift+arrow adds inside a block, and never a re-parse.
///
/// MUTATION ①: key the source on the caret's block alone and the first
/// assertion goes red — Shift+Down into the next paragraph leaves it rendered.
/// MUTATION ②: key it on the selection's bytes rather than its blocks and the
/// second goes red — every Shift+arrow re-lays-out the page.
#[test]
fn a_selection_reaching_another_block_is_a_layout_change_and_not_a_parse_change() {
    let source = "# head\n\nfirst paragraph\n\nsecond paragraph\n";
    let mut buffer = preview::PreviewBuffer::new(
        preview::PreviewSource::file(r"C:\w\live.md"),
        "live.md".to_owned(),
    );
    buffer.accept(preview::HeadOutcome::Read {
        text: source.to_owned(),
        truncated: false,
        mtime: None,
        content_says_text: true,
        encoding: preview::HeadEncoding::Utf8,
        lossy: false,
    });
    let (blocks, ranges, maps) = preview::parse_markdown_mapped(source);
    let key = |anchor: usize, caret: usize| {
        let caret = preview_edit::EditCaret {
            anchor,
            caret,
            ..preview_edit::EditCaret::default()
        };
        preview_document_key(
            &buffer,
            false,
            1200.0,
            1.0,
            PageArtKey {
                math_generation: 0,
                body_ink: [0, 0, 0],
                picture_generation: 0,
                picture_reach: PictureReach::from_the_top(),
                theme: bt_render::Theme::Dark,
            },
            preview_live::selection_span(source, &blocks, &ranges, &maps, &caret, None),
        )
    };
    let start = ranges[1].start + 2;
    let within = key(start, start + 4);
    let across = key(start, ranges[2].start + 3);
    assert_ne!(
        within, across,
        "① the selection reached the second paragraph, so it is drawn as source"
    );
    assert_ne!(
        across,
        key(ranges[2].start + 3, ranges[2].start + 3),
        "① and it is not the caret's block alone: the first paragraph stays source",
    );
    assert_eq!(within.parse, across.parse, "and nothing is parsed again");
    assert_eq!(
        within,
        key(start, start + 9),
        "② a selection growing inside its own block is not a layout change",
    );
    assert_eq!(
        within,
        key(start, start),
        "and neither is letting go of it: the caret's block is the same one",
    );
}

/// PIN — **`h1` and `h2` are painted with a hairline under them, and no
/// other level is** (`h1, h2 { border-bottom: 1px solid }`, §7.1.3i).
///
/// The rule is a quad in `--border`, drawn at the very bottom of the block's
/// own box, and the heading's text box stops short of it by exactly the
/// extent the measuring pass reserved — a rule painted into space nobody
/// reserved is a rule printed over the first line of the next paragraph.
///
/// MUTATION: drop the `- rule` from the heading's text rectangle and the
/// last assertion goes red, which is a title sitting on its own underline.
#[test]
fn the_first_two_heading_levels_are_painted_with_a_rule_under_them() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 600.0, 900.0];
    let blocks = preview::parse_markdown("# One\n\n## Two\n\n### Three\n");
    assert_eq!(blocks.len(), 3, "three headings and nothing else");
    let mut layout = Vec::new();
    let mut top = 0.0_f32;
    for level in 1..=3u8 {
        let height = metrics.heading_line_height(level) + metrics.heading_rule_extent(level);
        layout.push(MarkdownBlockLayout {
            top,
            ..MarkdownBlockLayout::solid(height)
        });
        top += height + metrics.heading_margin_bottom;
    }
    let layout: preview_viewport::Layout = layout.into();
    let built = markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        (&blocks, &layout),
        &palette,
    );
    let (left, right) = preview::markdown_measure_box(body, metrics);
    let origin = body[1] + metrics.padding_y;

    let rules: Vec<&bt_render::PreviewQuad> = built
        .quads
        .iter()
        .filter(|quad| quad.color == palette.preview_grid_line)
        .collect();
    assert_eq!(
        rules.len(),
        2,
        "one under the h1, one under the h2, and no more"
    );
    for (index, rule) in rules.iter().enumerate() {
        let placed = &layout[index];
        let bottom = origin + placed.top + placed.height;
        assert_eq!(
            rule.rect,
            [left, bottom - metrics.heading_rule_thickness, right, bottom],
            "the rule for level {} runs the column's own width at the foot of \
                 its block",
            index + 1
        );
    }
    for (index, paragraph) in built.paragraphs.iter().enumerate() {
        let level = index as u8 + 1;
        let placed = &layout[index];
        assert_eq!(
            paragraph.rect[3],
            origin + placed.top + placed.height - metrics.heading_rule_extent(level),
            "level {level}'s text stops above its own rule, not on it"
        );
        assert_eq!(paragraph.font_size_px, metrics.heading_font(level));
        assert_eq!(paragraph.line_height_px, metrics.heading_line_height(level));
    }
}

/// PIN — **a quoted line steps back to the muted ink; its bar keeps the
/// accent, and the styles that carry their own ink keep theirs**
/// (`blockquote { color: muted }`, §7.1.3i).
///
/// The ink is decided from the *span's style*, not from the colour already on
/// the run, which is what lets a code span inside a quote stay the fence's
/// own ink and a link stay the accent — a blanket recolour of the paragraph
/// would swallow both.
///
/// MUTATION: recolour every run and the code span's assertion goes red;
/// recolour none and the first two go red, which is the report's "quotes read
/// like body text with a stripe beside them".
#[test]
fn a_quoted_line_steps_back_to_the_muted_ink_and_keeps_its_accent_bar() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 600.0, 600.0];
    let blocks = preview::parse_markdown("> quoted **strongly** with `code` in it\n");
    let layout: preview_viewport::Layout = vec![MarkdownBlockLayout::rows(
        vec![metrics.line_height],
        metrics.quote_padding_y * 2.0,
    )]
    .into();
    let built = markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        (&blocks, &layout),
        &palette,
    );
    let (left, _) = preview::markdown_measure_box(body, metrics);

    let bar = built.quads.first().expect("a quote draws its bar first");
    assert_eq!(bar.color, palette.accent, "the bar keeps the accent");
    assert_eq!(bar.rect[2] - bar.rect[0], metrics.quote_bar);
    assert_eq!(bar.rect[0], left);

    let line = &built.paragraphs[0];
    assert_eq!(
        line.rect[0],
        left + metrics.quote_indent,
        "the text starts beside the bar, `padding: 0 15px` away from it"
    );
    let plain: Vec<[u8; 3]> = line
        .runs
        .iter()
        .filter(|run| !run.mono)
        .map(|run| run.color)
        .collect();
    assert!(
        plain.iter().all(|color| *color == palette.files_row_muted),
        "prose and emphasis alike step back: {plain:?}"
    );
    let code = line
        .runs
        .iter()
        .find(|run| run.mono)
        .expect("the fixture quotes a code span");
    assert_eq!(
        code.color, palette.preview_code_text,
        "and code keeps its own"
    );
    assert_eq!(code.font_scale, preview::PREVIEW_MD_CODE_FONT_RATIO);
}

/// PIN (user ruling, 2026-08-15) — **the read-only fact hangs on the path
/// foot's right hand, and the document gets back the bar it used to lose.**
///
/// The report was about the picture: on `huge.txt` the pane ended in two
/// 28px strips stacked on each other, the read-only bar sitting on the path
/// bar, and two horizontal rules of identical height at the bottom of one
/// pane read as a mistake however each one was justified on its own.
///
/// So the upper strip retired and the fact it carried moved into the lower
/// one's right hand. Three things had to become true and all three are here:
///
/// ① the wording is a **phrase** and not a sentence, because it now shares
///    28 pixels with a path — and it still says the size, which is the half
///    of the old sentence that was information;
/// ② the phrase stands flush against the strip's right edge with the path
///    stopping a gap short of it, so the two are two runs and not one;
/// ③ the body the document is laid out in is the pane's body **whole** — the
///    28 pixels the bar used to reserve are the document's again.
///
/// MUTATION for ①: give the constant back its old sentence and the size
/// assertion goes red on a phrase that no longer names 64 KB.
/// MUTATION for ②: hand `foot_notice_split` the run unchanged (return
/// `(run, run)`) and the disjointness assertion goes red — the path prints
/// through the phrase, which is the collision the split exists to prevent.
/// MUTATION for ③ is a **compile error**, and that is the point:
/// `bt_render::PreviewBody` no longer has a slot for furniture at all, so
/// the reservation cannot be reintroduced by an edit to one branch. The
/// height it used to take is pinned one layer down, by
/// `seats::a_preview_pane_reserves_its_path_strip_out_of_its_body`, which
/// asserts the pane gives up exactly one strip.
#[test]
fn the_read_only_fact_hangs_on_the_path_foots_right_hand() {
    // ① The phrase, and the size it names.
    assert_eq!(
        preview::preview_truncated_notice(),
        format!(
            "Read-only · {}",
            preview::format_byte_size(preview::PREVIEW_HEAD_BYTES as u64)
        ),
        "the phrase names the head read's real size, not a number typed twice"
    );

    // ② The division of the strip.
    let run = [100.0, 400.0, 500.0, 428.0];
    let gap = 12.0;
    let words = seats::dress_foot(
        seats::FootDress {
            dissolved: 0.0,
            run,
            lead: r"C:\w\huge.txt",
            flash: None,
            notice: preview::preview_truncated_notice(),
            cut: seats::LeadCut::Front,
            font_px: 10.0,
            gap_px: gap,
        },
        &mut ruler,
    );
    assert_eq!(words.notice, preview::preview_truncated_notice());
    assert_eq!(
        words.notice_box[2], run[2],
        "flush with the strip's right edge"
    );
    assert_eq!(
        words.notice_box[2] - words.notice_box[0],
        ruler(preview::preview_truncated_notice(), 10.0),
        "and no wider than its own words"
    );
    assert_eq!(
        words.lead_box[2],
        words.notice_box[0] - gap,
        "the path stops a gap short of the phrase"
    );
    assert!(
        words.lead_box[2] < words.notice_box[0],
        "two runs, not one: nothing of the path is inside the phrase's box"
    );
    assert!(!words.flashing, "nothing is being confirmed");
}

/// ⑥ A same-length edit rebuilds the document.
///
/// **The named bug.** The cache key's revision counter was the content's
/// *length* — written down as "the cheapest honest revision counter until
/// slice 3 gives a buffer edits to count" — and a length cannot see the
/// commonest edit there is. Typing over a selected character, correcting a
/// letter, pressing Delete then typing the replacement: all three keep the
/// length, and all three left the cached document convinced it was still
/// looking at the text from before the keystroke.
///
/// Mutation: put `content_len: buffer.content.map_or(0, str::len)` back in
/// place of `revision` in [`preview_document_key`]. The first assertion then
/// fails and the second still passes, which is exactly the shape of the bug.
#[test]
fn a_same_length_edit_rebuilds_the_cached_document() {
    let mut buffer = text_buffer("a.rs", "let x = 1;\n");
    let before = document_key(&buffer, false, 400.0, 2.0);
    buffer.edit_content(|content| {
        content.replace_range(8..9, "2");
        true
    });
    assert_eq!(buffer.content.as_deref(), Some("let x = 2;\n"));
    let after = document_key(&buffer, false, 400.0, 2.0);
    assert_ne!(
        before, after,
        "one letter for another is still a different document"
    );
    // And nothing else moved: the same buffer at the same width and scale is
    // the same key, or every wheel notch would re-parse the file.
    assert_eq!(after, document_key(&buffer, false, 400.0, 2.0));
    assert_ne!(after, document_key(&buffer, true, 400.0, 2.0));
    assert_ne!(after, document_key(&buffer, false, 401.0, 2.0));
    assert_ne!(after, document_key(&buffer, false, 400.0, 1.0));
}

/// ⑤ A caret, a selection and a scroll survive a switch to another buffer
/// and back.
///
/// Ruling 8⑧, and the half of it a window that never rebuilds its DOM still
/// has to earn: the prototype's problem was a re-render destroying the live
/// `<textarea>`, and this one's is the *switcher* — a pool whose whole point
/// is that "unsaved edits survive switching with zero prompts" would be
/// telling half a truth if the place you were reading did not survive with
/// them.
///
/// Mutation: make [`PreviewViewStore::remember`] a no-op, or have
/// [`PreviewViewStore::restore`] always answer with the default.
#[test]
fn a_caret_and_a_scroll_survive_a_switch_to_another_buffer_and_back() {
    let mut store = PreviewViewStore::default();
    let one = &preview::PreviewSource::file(r"C:\w\one.rs");
    let two = &preview::PreviewSource::file(r"C:\w\two.rs");
    // A file never looked at starts at the top with the caret at its head.
    assert_eq!(store.restore(one), PreviewViewState::default());

    let reading = PreviewViewState {
        caret: preview_edit::EditCaret {
            anchor: 40,
            caret: 12,
            desired_column: Some(7),
            desired_x: None,
        },
        scroll: [16.0, 380.0],
    };
    store.remember(one, reading);
    // Away to another file, which has a place of its own.
    store.remember(
        two,
        PreviewViewState {
            caret: preview_edit::EditCaret {
                anchor: 3,
                caret: 3,
                desired_column: None,
                desired_x: None,
            },
            scroll: [0.0, 19.0],
        },
    );
    assert_eq!(
        store.restore(one),
        reading,
        "and back to the first: the caret, what it had selected, and how far down"
    );
    assert_eq!(store.restore(two).scroll, [0.0, 19.0]);
    assert_eq!(store.restore(one).caret.range(), 12..40);
}

/// RED ② — **a body with unsaved edits is never overwritten, and the window
/// says so** (user ruling 2026-08-29).
///
/// The standing half is `mark_stale`'s: the person's text is the newer of
/// the two. What the ruling adds is that the disagreement is *reported at
/// the moment it happens* rather than kept back until somebody presses save,
/// and that the two answers a person can give it are on the strip beside it.
///
/// Both verbs, because they are the two halves of one promise: `Keep my
/// edits` must leave every character where it is, and `Reload` must be the
/// only thing in this product that throws them away.
///
/// RED GATE ①: let `note_disk_moved` fall through to `mark_stale` for a
/// dirty buffer and the first block fails — the edits are gone and nothing
/// was said, which is the outcome the ruling exists to forbid. RED GATE ②:
/// make [`preview::PreviewBuffer::keep_this_body`] clear `dirty` as well and
/// the second block fails. RED GATE ③: make
/// [`preview::PreviewBuffer::take_the_disks_copy`] leave `dirty` standing
/// and the third block fails: `mark_stale` refuses a dirty buffer, so
/// `Reload` would take the strip down and change nothing.
#[test]
fn a_body_with_unsaved_edits_is_not_overwritten_and_says_so() {
    let dir = disk_scratch("dirty");
    let path = dir.join("notes.md");
    std::fs::write(&path, "# one\n").expect("write");
    let mut buffer = buffer_read_from(&path);
    assert!(buffer.edit_content(|text| {
        text.push_str("typed by hand\n");
        true
    }));
    assert!(buffer.dirty);

    // ① Somebody else writes the file.
    std::fs::write(&path, "# two\n").expect("rewrite");
    move_the_disk_forward(&path);
    assert_eq!(
        buffer.note_disk_moved(true, preview::file_mtime(&path)),
        preview::DiskVerdict::Say,
        "a strip, not a read"
    );
    assert_eq!(buffer.disk, preview::DiskNews::Changed);
    assert!(
        !buffer.is_behind_the_disk(),
        "nothing is owed to the worker: the body on the glass is not going anywhere"
    );
    assert!(
        buffer
            .content
            .as_deref()
            .is_some_and(|body| body.contains("typed by hand")),
        "and the words that were typed are still there"
    );
    assert_eq!(
        buffer.note_disk_moved(true, preview::file_mtime(&path)),
        preview::DiskVerdict::Nothing,
        "a second notification about the same disagreement is not a second frame"
    );
    assert_eq!(
        notice::Notice::DiskChanged.verbs(),
        &[
            notice::NoticeVerb::KeepMyEdits,
            notice::NoticeVerb::ReloadFromDisk
        ],
        "and the strip offers exactly the two answers a person can give it"
    );

    // ② Keep my edits: the strip goes down, the text stays, and the save is
    // still guarded.
    assert!(buffer.keep_this_body());
    assert_eq!(buffer.disk, preview::DiskNews::Level);
    assert!(buffer.dirty, "keeping is not saving");
    assert!(
        buffer
            .content
            .as_deref()
            .is_some_and(|body| body.contains("typed by hand"))
    );
    assert_eq!(
        buffer.save(),
        preview::SaveOutcome::Conflict,
        "ruling 8-9 still stands: dismissing the strip is not permission to overwrite"
    );

    // ③ Reload: the one door that discards, and it really does read again.
    assert_eq!(
        buffer.note_disk_moved(true, preview::file_mtime(&path)),
        preview::DiskVerdict::Say
    );
    assert!(buffer.take_the_disks_copy(), "a head read is now owed");
    assert!(!buffer.dirty);
    assert_eq!(buffer.disk, preview::DiskNews::Level);
    buffer.accept(preview::read_head(&path));
    assert_eq!(buffer.content.as_deref(), Some("# two\n"));

    let _ = std::fs::remove_dir_all(&dir);
}

/// RED ③ — **a file that is deleted takes nothing with it** (user ruling
/// 2026-08-29).
///
/// Before the ruling a delete arrived at `mark_stale` like any other change,
/// and the head read it asked for came back `Refused(Fault)` — so the window
/// answered somebody else's `rm` by replacing the document a reader was
/// looking at with "no such file". The buffer is what the reader has left of
/// that file, and it is kept.
///
/// **No verb**, and that is the ruling read honestly: there is nothing to
/// reload and nothing to choose between, so the strip carries the sentence
/// and the `×` that says it has been read.
///
/// RED GATE: give `note_disk_moved`'s absent arm the clean arm's body — a
/// `mark_stale` and a read — and both halves fail: the words go, and a
/// window that was showing a document is showing a fault card.
#[test]
fn a_deleted_file_keeps_the_body_that_was_read_from_it() {
    let dir = disk_scratch("gone");
    let path = dir.join("notes.md");
    std::fs::write(&path, "# still here\n").expect("write");
    let mut buffer = buffer_read_from(&path);
    std::fs::remove_file(&path).expect("delete");

    assert_eq!(
        buffer.note_disk_moved(false, None),
        preview::DiskVerdict::Say
    );
    assert_eq!(buffer.disk, preview::DiskNews::Deleted);
    assert!(
        !buffer.is_behind_the_disk(),
        "nothing is asked of a disk that has nothing to answer with"
    );
    assert_eq!(
        buffer.content.as_deref(),
        Some("# still here\n"),
        "what the reader was reading is what the reader is still reading"
    );
    assert!(notice::Notice::DiskDeleted.verbs().is_empty());

    // And a file that comes back is the ordinary case again.
    std::fs::write(&path, "# back\n").expect("recreate");
    // A file recreated inside one of NTFS's ticks carries the very mtime
    // this buffer is already holding, and a window that cannot tell those
    // apart is the resolution the save path has always lived with (ticket
    // T-EDIT-DISK). The disk is moved by hand so the test reads the rule
    // and not the clock.
    move_the_disk_forward(&path);
    assert_eq!(
        buffer.note_disk_moved(true, preview::file_mtime(&path)),
        preview::DiskVerdict::ReadAgain,
        "the sentence comes down and the bytes are asked for, in one move"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **P126/P168 — a preview pane crossing into another tab arrives holding
/// the edit it left with.**
///
/// The comment this replaces said the opposite in as many words: "only the
/// picture: a document is a buffer, and buffers live in the tab's pool which
/// does not migrate". §7.1.3 rules that it does, and what the old behaviour
/// cost is exactly this test — a pane with unsaved work in it changed tabs
/// and arrived pointing at a path its new tab had no buffer for, so the
/// edits were still in memory and nothing on screen could reach them.
///
/// The **same object**, which is what the ruling asks for and what these
/// assertions are: the body, the dirty bit and the revision are the ones that
/// left, not a fresh read of the same file. And the view goes with the
/// surface (ruling 8⑧) — scroll, caret and the markdown flip are the pane's,
/// so they travel with the pane rather than being reset by the move.
///
/// MUTATION ①: carry only `pane.image` across (the pre-slice behaviour) and
/// every content assertion goes red while the terminal beside it stays green
/// — which is exactly how the bug reached a release.
#[test]
fn a_preview_pane_changing_tabs_arrives_holding_the_edit_it_left_with() {
    let path = preview::PreviewSource::file(r"D:\notes\todo.txt");
    let mut edited = buffer_saying(r"D:\notes\todo.txt", "todo.txt", "milk\nbread\n");
    edited.dirty = true;
    let revision_before = edited.revision;

    let (mut source, seat) = tab_with_a_preview(1, vec![edited]);
    {
        let pane = source.preview_panes.entry(source.preview_here(seat));
        pane.scroll = [0.0, 96.0];
        pane.caret.anchor = 6;
        pane.caret.caret = 6;
        pane.md_source = true;
    }
    let arrived = arriving_as(&source, 90);
    let landed = arrived
        .iter()
        .find(|(was, _)| *was == seat)
        .map(|(_, now)| *now)
        .expect("the preview seat is in the tree");

    let mut target = cross_tab(2, &["ALPHA"]);
    absorb_tab_sessions(&mut source, &mut target, &arrived);

    let pane = target
        .preview_panes
        .get(target.preview_here(landed))
        .expect("the view arrived under the id the pane now answers to");
    assert_eq!(
        pane.buffer.as_ref(),
        Some(&path),
        "the pane still knows which file it was on"
    );
    assert_eq!(pane.scroll, [0.0, 96.0], "and how far down it was in it");
    assert_eq!(pane.caret.caret, 6, "and where the caret was");
    assert!(pane.md_source, "and which face of the file it was reading");

    let buffer = target
        .preview_pool
        .get(&path)
        .expect("the buffer it names is in the pool it arrived in");
    assert!(
        buffer.dirty,
        "the unsaved work is still unsaved, not re-read"
    );
    assert_eq!(buffer.content.as_deref(), Some("milk\nbread\n"));
    assert_eq!(
        buffer.revision, revision_before,
        "the same object: an uncontested arrival is not a content change"
    );
    assert!(
        source
            .preview_panes
            .get(source.preview_here(seat))
            .is_none(),
        "and nothing of it is left behind to be shown twice"
    );
}

/// **P122/P127 — two pools become one: one buffer per file, dirty wins, a
/// tie stays with the tab that was already holding it.**
///
/// Asserted through the gesture rather than only on the pool, because the
/// second half of the clause is about *panes*: "在显 loser 的 pane 重指向
/// winner". Both tabs here have a pane on `notes.md`, and after the merge
/// there is one buffer and both panes read it.
///
/// MUTATION ③: make the arrival win unconditionally (drop the
/// `incoming.dirty && !twin.dirty` guard in `merge_buffer`) and the clean
/// copy walks over the target's unsaved work — the `y` assertions go red.
#[test]
fn merging_two_tabs_leaves_one_buffer_per_file_and_the_dirty_one_wins() {
    let notes = preview::PreviewSource::file(r"D:\notes\notes.md");
    let plan = preview::PreviewSource::file(r"D:\notes\plan.md");

    let mut arriving_notes = buffer_saying(r"D:\notes\notes.md", "notes.md", "ARRIVING EDIT");
    arriving_notes.dirty = true;
    let arriving_plan = buffer_saying(r"D:\notes\plan.md", "plan.md", "clean arrival");
    let (mut source, _) = tab_with_a_preview(1, vec![arriving_notes, arriving_plan]);

    let staying_notes = buffer_saying(r"D:\notes\notes.md", "notes.md", "on disk");
    let mut staying_plan = buffer_saying(r"D:\notes\plan.md", "plan.md", "STAYING EDIT");
    staying_plan.dirty = true;
    let (mut target, host) = tab_with_a_preview(2, vec![staying_notes, staying_plan]);
    let host_revision = target
        .preview_pool
        .get(&notes)
        .expect("the host is holding it")
        .revision;

    let arrived = arriving_as(&source, 90);
    absorb_tab_sessions(&mut source, &mut target, &arrived);

    assert_eq!(
        target.preview_pool.len(),
        2,
        "two files, two buffers — one each, not four"
    );
    let merged = target.preview_pool.get(&notes).expect("notes.md survived");
    assert_eq!(
        merged.content.as_deref(),
        Some("ARRIVING EDIT"),
        "dirty wins: the unsaved copy is the one that stands"
    );
    assert!(merged.dirty);
    assert!(
        merged.revision > host_revision,
        "and every surface on that path is told the body under it changed"
    );

    let held = target.preview_pool.get(&plan).expect("plan.md survived");
    assert_eq!(
        held.content.as_deref(),
        Some("STAYING EDIT"),
        "and a clean arrival never walks over unsaved work already here"
    );
    assert!(held.dirty);

    assert_eq!(
        target
            .preview_panes
            .get(target.preview_here(host))
            .and_then(|pane| pane.buffer.clone()),
        Some(notes.clone()),
        "the host's own pane was on the loser and now reads the winner — a \
             surface names its buffer by source, so one buffer per file *is* the \
             redirect"
    );
}

/// The law itself, on the pool alone — every branch, including the two the
/// gesture above cannot reach in one merge.
///
/// A tie between two dirty copies is the case §7.1.3 declines to arbitrate
/// ("同文件跨 tab 的并发编辑留给产品端磁盘冲突检测"), and the honest answer
/// until that detection exists is to keep the copy that is already here
/// rather than to overwrite unsaved work with unsaved work.
#[test]
fn the_pool_merge_law_prefers_dirt_then_the_incumbent_and_appends_the_rest() {
    let mut target = preview::PreviewPool::default();
    target.insert(buffer_saying(r"D:\a.txt", "a.txt", "clean here"));
    target.insert(buffer_saying(r"D:\b.txt", "b.txt", "also clean here"));

    // Two clean copies: nothing is at stake, so nothing moves — not even the
    // revision, which would make every pane on it rebuild for no reason.
    let untouched = target
        .get(&preview::PreviewSource::file(r"D:\a.txt"))
        .unwrap()
        .revision;
    target.merge_buffer(buffer_saying(r"D:\a.txt", "a.txt", "clean there"));
    let a = target
        .get(&preview::PreviewSource::file(r"D:\a.txt"))
        .unwrap();
    assert_eq!(a.content.as_deref(), Some("clean here"));
    assert_eq!(a.revision, untouched);

    // Two dirty copies: the one already here stands.
    let mut here = buffer_saying(r"D:\b.txt", "b.txt", "DIRTY HERE");
    here.dirty = true;
    target.insert(here);
    let mut there = buffer_saying(r"D:\b.txt", "b.txt", "DIRTY THERE");
    there.dirty = true;
    target.merge_buffer(there);
    assert_eq!(
        target
            .get(&preview::PreviewSource::file(r"D:\b.txt"))
            .unwrap()
            .content
            .as_deref(),
        Some("DIRTY HERE"),
        "a tie stays with the incumbent, so nobody's unsaved work is discarded"
    );

    // A file this pool has never seen is simply history it now has.
    target.merge_buffer(buffer_saying(r"D:\c.txt", "c.txt", "new to me"));
    assert_eq!(target.len(), 3);

    // And the winner keeps the loser's place in the history, rather than
    // jumping to the front of a list the switcher reads in order.
    let mut winner = buffer_saying(r"D:\a.txt", "a.txt", "DIRTY THERE");
    winner.dirty = true;
    target.merge_buffer(winner);
    assert_eq!(
        target
            .buffers()
            .map(|buffer| buffer.name.as_str())
            .collect::<Vec<_>>(),
        vec!["a.txt", "b.txt", "c.txt"]
    );
}

/// RED — **the shell page is gone, and so is everything that existed to
/// serve it** (user ruling 2026-08-28; `docs/DESIGN.md` §7.44 ④).
///
/// The ruling that retired route A named five things, and a retirement that
/// left any one of them standing would be a build carrying a second, dead
/// way to play a video — which is exactly the state §7.23's own note warned
/// this slice about when it wrote "一行都没有删".
///
/// 1. **`player.rs` is not one of this crate's files**, and is not lying
///    undeclared beside them either. The module that wrote the page.
/// 2. **No opening video element tag anywhere in the crate.** The element
///    the page existed to contain, and the one string that could not survive
///    by accident. Named in prose and not spelled here: this doc comment is
///    inside one of the files read, and a reader who has to work out that a
///    comment is not counted has been given something to work out.
/// 3. **`Mint::VideoShell` does not exist.** The note the gate carried about
///    a page standing in for a recording.
/// 4. **No autoplay-policy argument.** It was written for one self-starting
///    player in one page this window wrote; `bt-platform`'s own gate pins
///    the other half.
/// 5. **Nothing writes `%LOCALAPPDATA%\Folio\player`.** The folder the
///    shells were content-addressed into.
///
/// **Files left behind by an older build are not swept, on purpose.** A
/// shell was about seven hundred bytes and content-addressed, so what a
/// reader who upgrades has is one small file per recording they ever played,
/// in a cache folder, inside a profile that goes when the profile goes. A
/// sweep would need a rule for the *other* Folio running against the same
/// `%LOCALAPPDATA%` — which is the reason there was never a sweep — and
/// deleting from a directory this build no longer knows about, on the
/// strength of a name pattern, is a worse thing to ship than seven hundred
/// stale bytes. Written down rather than done.
///
/// RED GATE: restore any one of the five and the assertion that names it
/// fails. The second is the load-bearing one — a build that kept the module
/// but stopped calling it would pass the other four.
#[test]
fn the_shell_page_is_gone() {
    let index = source();
    let source_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let relative = |path: &std::path::Path| {
        bt_source::normalized(path)
            .strip_prefix(bt_source::normalized(&source_dir))
            .expect("a file under this crate's own src")
            .to_string_lossy()
            .replace('\\', "/")
    };

    // ── ① the module that wrote the page ──────────────────────────────────
    //
    // Twice, because a file this crate does not declare is still a file: no
    // file the crate is made of is named `player.rs`, and neither is any `.rs`
    // file lying on the disk beside the declarations. `Index::cross_check` is
    // where the second kind is reported rather than lost, and the two together
    // are wider than the `src/player.rs` existence check they replace, which
    // never looked inside a subdirectory.
    let is_the_module = |path: &std::path::Path| path.ends_with("player.rs");
    assert!(
        !index.files().iter().any(|file| is_the_module(file.path())),
        "the module that wrote the shell page is declared again"
    );
    assert!(
        !index
            .cross_check()
            .only_on_disk
            .iter()
            .any(|path| is_the_module(path)),
        "the module that wrote the shell page is lying beside the declarations:\n{}",
        index.cross_check().report()
    );

    assert!(
        index.files().len() > 40,
        "the declarations found the crate: {}",
        index.files().len()
    );

    // ── ②–⑤, asked of every file the crate is made of ────────────────────
    //
    // **Asked of the code, and comments are dropped before it is asked**
    // (2026-08-28).
    //
    // Not a loophole: the rule is about what this crate *does*, and a
    // paragraph explaining a route that was retired is not that route.
    // `preview.rs` carries four sentences about the page, `webhost.rs` one
    // about the accessor that read its mint, and this test's own doc comment
    // is another — a pin that forbade the prose would forbid the only record
    // of why the code is gone. `bt-render`'s source pins drop comments for
    // exactly this reason, and `View::CodeKeepingLiterals` is that reading
    // with the literals kept, which is where three of these four spellings
    // would live if they came back.
    //
    // The needles are spelled whole. This file is one of the files read, and
    // a needle written whole used to be found by the array looking for it —
    // which is why they were assembled from halves; `needle!` records where
    // the expression that built it stands and excludes that one span (§2.6),
    // which is the same rule said once instead of four times.
    let mut said: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut elements: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for needle in [
        needle!(Pattern::text("VideoShell")),
        needle!(Pattern::text("--autoplay-policy")),
        needle!(Pattern::text("mint_player_shell")),
        needle!(Pattern::text("shell_html")),
    ] {
        let spelling = needle.pattern().spelling();
        let answer = found(needle, View::CodeKeepingLiterals);
        println!("{}", answer.report(index));
        for occurrence in answer.occurrences() {
            let file = index
                .file_at(occurrence.span.start())
                .expect("every match stands in a file of the universe");
            said.insert(format!("{spelling} is still in {}", relative(file.path())));
        }
    }
    // **And the element, shaped like a tag rather than like four
    // characters.** `Option<video_seat::BarLayout>` contains `<` followed by
    // `video` and is a Rust type; a module named after the thing that
    // replaced the page is not the page coming back. What this looks for
    // cannot be written except by writing the element, and it is the whole
    // class: the bare tag, the tag with attributes, and the self-closing
    // one alike.
    let openings = found(needle!(Pattern::text("<video")), View::CodeKeepingLiterals);
    println!("{}", openings.report(index));
    for occurrence in openings.occurrences() {
        let file = index
            .file_at(occurrence.span.start())
            .expect("every match stands in a file of the universe");
        let text = index.text(file.span());
        let path = relative(file.path());
        *elements.entry(path.clone()).or_default() += 1;
        let after = text[occurrence.span.end() - file.span().start()..]
            .chars()
            .next();
        if matches!(after, None | Some('>' | ' ' | '\t' | '\n' | '/')) {
            said.insert(format!("an opening video element is back in {path}"));
        }
    }
    // And the folder the shells lived in is written by nothing. The spelling
    // is the one a `\` in a Rust literal is written with, which is what both
    // readings look for in the bytes of a file.
    let folder = found(
        needle!(Pattern::text("Folio\\player")),
        View::CodeKeepingLiterals,
    );
    println!("{}", folder.report(index));
    for occurrence in folder.occurrences() {
        let file = index
            .file_at(occurrence.span.start())
            .expect("every match stands in a file of the universe");
        said.insert(format!(
            "the shell folder is still named in {}",
            relative(file.path())
        ));
    }

    println!("the element's spelling stands in: {elements:#?}");
    assert!(said.is_empty(), "{said:#?}");
}
