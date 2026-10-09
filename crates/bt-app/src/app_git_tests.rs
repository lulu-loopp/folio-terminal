//! **The crate root: git panel and graph.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::host_path;
use winit::keyboard::{Key, NamedKey};

/// V14 — the graph's six keys, and only those six.
///
/// The negative half is the half worth writing down: every key not in this
/// list has to reach the scroll below it, because a focused preview's
/// `PageDown` is still a page down and its letters are still swallowed by
/// the surface that already swallows them. A translation that claimed one
/// key too many would take a verb away from a surface underneath and there
/// would be nothing on screen to say so.
#[test]
fn a_focused_graph_answers_six_keys_and_leaves_every_other_one_alone() {
    use winit::keyboard::SmolStr;
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::ArrowUp), ModifiersState::empty()),
        Some(git_graph::GraphKey::Up)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty()),
        Some(git_graph::GraphKey::Down)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::Home), ModifiersState::empty()),
        Some(git_graph::GraphKey::Home)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::End), ModifiersState::empty()),
        Some(git_graph::GraphKey::End)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::Enter), ModifiersState::empty()),
        Some(git_graph::GraphKey::Enter)
    );
    assert_eq!(
        graph_key_of(&Key::Named(NamedKey::Escape), ModifiersState::empty()),
        Some(git_graph::GraphKey::Escape)
    );
    for other in [
        Key::Named(NamedKey::PageDown),
        Key::Named(NamedKey::PageUp),
        Key::Named(NamedKey::ArrowLeft),
        Key::Named(NamedKey::ArrowRight),
        Key::Named(NamedKey::Space),
        Key::Named(NamedKey::Tab),
        Key::Character(SmolStr::new("g")),
    ] {
        assert_eq!(
            graph_key_of(&other, ModifiersState::empty()),
            None,
            "{other:?} is not the graph's"
        );
    }
    // And none of the six is a **bare** row of the chord registry — see
    // [`graph_key_of`] for why a seat-local key is not a binding.
    //
    // Bare, and not "the key at all" (2026-08-16): the registry matches its
    // modifiers exactly, so `Ctrl+Shift+↑` walking the command marks and `↑`
    // walking a graph's rows are two different presses and neither can reach
    // the other. What the graph's keys may not survive is a row claiming the
    // *same* press, which is what this asserts.
    //
    // **`Scope::SearchOpen` is exempt, and the ladder is why** (§7.7, W2
    // slice ④). The one row that claims a bare key on this list is
    // `close-search`, and its scope is exactly the state in which §7.1.5's
    // Escape ladder has *already* taken the key: `close_search` answers at
    // its own rung, which stands whole screens above `preview_key` where the
    // graph's rung is. So the row is never the thing that takes an Escape
    // from a graph — with a capsule up, the graph did not have it before
    // this row existed either. What the assertion protects is a row taking
    // one of the six in a state where the ladder would have left it alone,
    // and that is still nobody's.
    for binding in shortcuts::BINDINGS {
        let Some(chord) = &binding.chord else {
            continue;
        };
        if binding.scope == shortcuts::Scope::SearchOpen {
            continue;
        }
        assert!(
            !matches!(
                chord.key,
                shortcuts::ChordKey::Named(
                    NamedKey::ArrowUp
                        | NamedKey::ArrowDown
                        | NamedKey::Home
                        | NamedKey::End
                        | NamedKey::Enter
                        | NamedKey::Escape
                )
            ) || chord.modifiers != ModifiersState::empty(),
            "{:?} claims a key the graph answers bare",
            binding.action
        );
    }
}

/// The same question, asked of the host that is not in a tab.
///
/// A float has no seat layout and no tab, so the two facts left are the
/// master switch and the page the window was left on — and a window keeps
/// its page while the switch is off, exactly as a column does (§7.1.6g ②:
/// the setting decides reachability, the view records the choice).
#[test]
fn a_floating_window_shows_the_page_it_was_torn_off_on() {
    assert!(float_git_page_shown(true, seats::FilesView::Git));
    assert!(
        !float_git_page_shown(false, seats::FilesView::Git),
        "the master switch decides whether the page is reachable at all"
    );
    assert!(
        !float_git_page_shown(true, seats::FilesView::Files),
        "and a window torn off the tree is still on the tree"
    );
}

/// **The prose block draws the file's own bytes at their own offsets**
/// (§7.1.3w) — which is what lets a press, a caret and a band be read
/// straight back as file offsets, with no provenance in between.
///
/// Every line is one paragraph, the paragraphs concatenate to the block's own
/// bytes with the file's own breaks between them, and the offset beside each
/// one is where that line begins **in the file**. Tabs are not expanded and
/// nothing is normalised: this is the file and not a rendering of it.
///
/// MUTATION: expand tabs the way the monospace face does and every offset
/// after the first tab on a line names the wrong byte.
#[test]
fn a_prose_block_draws_the_files_own_bytes_at_their_own_offsets() {
    let file = "intro\n\n- \tone **two**\n- 三 four\n";
    let block = file.find("- \t").expect("the fixture has a list in it");
    let text = "- \tone **two**\n- 三 four";
    let prose = MarkdownProseBlock {
        index: 1,
        range: block..file.len(),
        lines: prose_source_lines(text),
        text: text.to_owned(),
        heading: false,
        font_size: 14.0,
        line_height: 20.0,
    };
    let palette = bt_render::chrome_palette();
    let paragraphs = markdown_prose_paragraphs(
        &prose,
        [10.0, 100.0, 210.0, 140.0],
        &[20.0, 20.0],
        None,
        &palette,
    );
    assert_eq!(paragraphs.len(), 2, "one paragraph per source line");
    for line in &paragraphs {
        let drawn: String = line
            .paragraph
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect();
        assert_eq!(
            &file[line.start..line.start + drawn.len()],
            drawn,
            "the line drawn at {} is the file's own bytes there",
            line.start,
        );
    }
    assert!(
        paragraphs[0].paragraph.runs[0].text.contains('\t'),
        "and a tab is drawn as the character it is, not as the spaces it stands in for",
    );
    // Stacked at the heights the measuring pass wrote down, and both
    // wrapped into the same column.
    assert!((paragraphs[0].paragraph.rect[1] - 100.0).abs() < f32::EPSILON);
    assert!((paragraphs[1].paragraph.rect[1] - 120.0).abs() < f32::EPSILON);
    assert!(
        paragraphs
            .iter()
            .all(|line| (line.paragraph.rect[0] - 10.0).abs() < f32::EPSILON
                && (line.paragraph.rect[2] - 210.0).abs() < f32::EPSILON)
    );
}

/// **The letters being composed are drawn where they are being typed, in
/// the block's own face** (user report 2026-09-12; adversarial review
/// 2026-09-11 finding A8).
///
/// The report: a paragraph of Chinese with the caret in it showed
/// `我们是天下第一好`, a bare caret, and the candidate list — and nothing at
/// all between the caret and the list, because the page drew the file and
/// the composition lived in `window.preedit` where only the text face and
/// the grid ever looked.
///
/// What closes it is the composition spliced into the very paragraph the
/// shaper is handed, which is what makes the letters land in the block's own
/// face beside the letters they were typed among, with the rest of the
/// sentence pushed along in front of them rather than drawn over.
///
/// MUTATION ①: drop the splice and the paragraph is the file's own bytes
/// again — the first assertion goes red, which is the report.
/// MUTATION ②: splice into `prose.text` instead of into the runs and the
/// last goes red: the composition would be in the block, an Escape would
/// have to un-type it, and the caret's own byte would have moved.
#[test]
fn a_composition_is_drawn_at_the_caret_in_a_prose_block() {
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
    let palette = bt_render::chrome_palette();
    let at = "我们是".len();
    // An input method that pre-edits latin, one that pre-edits Han, and the
    // apostrophe'd reading a Chinese method actually shows mid-word.
    for composing in ["nikan", "你看", "ni'kan"] {
        let preedit = MarkdownPreedit {
            text: composing.to_owned(),
            caret_byte: composing.len(),
        };
        let lines = markdown_prose_paragraphs(
            &prose,
            [10.0, 100.0, 210.0, 120.0],
            &[20.0],
            Some((at, &preedit)),
            &palette,
        );
        let [line] = lines.as_slice() else {
            panic!("one source line: {lines:#?}", lines = lines.len());
        };
        let drawn: String = line
            .paragraph
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect();
        assert_eq!(
            drawn,
            format!("我们是{composing}天下第一好"),
            "the composition stands at the caret, among the letters it is being typed into",
        );
        assert_eq!(line.paragraph.runs.len(), 3, "head, composition, tail");
        assert_eq!(line.paragraph.runs[1].text, composing);
        let face = &line.paragraph.runs[0];
        assert!(
            line.paragraph.runs.iter().all(|run| run.mono == face.mono
                && run.bold == face.bold
                && run.italic == face.italic
                && (run.font_scale - face.font_scale).abs() < f32::EPSILON
                && run.color == face.color),
            "and it is set in the block's own face, not in a fourth one",
        );
        assert_eq!(
            line.splice,
            Some(preview_live::ProseSplice {
                at,
                len: composing.len(),
                caret: composing.len(),
            }),
            "and the measuring pass is told where the letters went in",
        );
        assert_eq!(
            prose.text, text,
            "and the block's own bytes are exactly what they were",
        );
    }

    // **The mixed line, with its marks showing** (§7.1.3w) — a composition
    // typed between the stars of `**预览**` splices there and nowhere else,
    // and the marks either side of it are still the characters they are.
    let marked = "**预览**窗格";
    let mixed = MarkdownProseBlock {
        range: 0..marked.len() + 1,
        lines: prose_source_lines(marked),
        text: marked.to_owned(),
        ..prose.clone()
    };
    let preedit = MarkdownPreedit {
        text: "shi".to_owned(),
        caret_byte: 3,
    };
    let lines = markdown_prose_paragraphs(
        &mixed,
        [10.0, 100.0, 210.0, 120.0],
        &[20.0],
        Some(("**预览**".len(), &preedit)),
        &palette,
    );
    let drawn: String = lines[0]
        .paragraph
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(drawn, "**预览**shi窗格");
}

/// PIN (R31's third invalidation moment) — **the two automatic triggers read
/// only what somebody is looking at, and the command one reads only the
/// repository the command was run in.**
///
/// The showing gate is the same one the first reading keeps
/// ([`columns_wanting_git`]) and it is kept a second time here on purpose: a
/// file changing is not on its own a reason to spend a subprocess. What is
/// new is the asymmetry between the two triggers, and it is the whole reason
/// there are two — a command end carries a folder to compare against, and a
/// window coming back does not, because what happened while it was away
/// happened in another process.
#[test]
fn a_command_and_a_focus_re_read_only_the_repositories_on_screen() {
    let page = SeatId(1);
    let tree = SeatId(2);
    let repo = host_path(r"D:\repo");
    let other = host_path(r"D:\other");
    let graph = host_path(r"D:\repo");
    let surfaces = vec![
        (GitOrigin::Column(page), repo.clone(), true),
        // Same tab, same window, on its Files page: available, not showing.
        (GitOrigin::Column(tree), other.clone(), false),
        (GitOrigin::Graph(graph.clone()), graph.clone(), true),
    ];

    assert_eq!(
        git_surfaces_wanting_reread(false, &surfaces, None),
        Vec::new(),
        "with the master switch off nothing is read, however many pages are up"
    );

    // B: the window came back. Every showing surface, and nothing else.
    assert_eq!(
        git_surfaces_wanting_reread(true, &surfaces, None),
        vec![GitOrigin::Column(page), GitOrigin::Graph(graph.clone())],
        "a column on its tree is not a surface looking at a repository"
    );

    // A: a command ended in a pane standing in the repository the page shows.
    assert_eq!(
        git_surfaces_wanting_reread(
            true,
            &surfaces,
            Some(&[host_path(r"D:\repo\crates\bt-app")])
        ),
        vec![GitOrigin::Column(page), GitOrigin::Graph(graph)],
        "a subdirectory of the root is inside the root"
    );
    assert_eq!(
        git_surfaces_wanting_reread(true, &surfaces, Some(std::slice::from_ref(&other))),
        Vec::new(),
        "a command that ended in the folder the *hidden* page is rooted at \
             reads nothing: that page is not showing, and the one that is shows \
             another repository"
    );
    assert_eq!(
        git_surfaces_wanting_reread(true, &surfaces, Some(&[host_path(r"D:\repository")])),
        Vec::new(),
        "and the folder next door whose name merely starts the same way is \
             not inside it"
    );
    assert_eq!(
        git_surfaces_wanting_reread(true, &surfaces, Some(&[])),
        Vec::new(),
        "no shell finished anywhere: nothing to re-read"
    );
}

/// PIN (v2 ④) — a repo-relative path becomes the path a person would type,
/// which is what the clipboard and Explorer are both handed.
///
/// git speaks forward slashes on every machine; `/select` and a pasted path
/// want the platform's own separator. One function, so the copy and the
/// reveal cannot spell the same file two ways.
#[test]
fn a_repo_relative_path_is_joined_in_the_grammar_the_platform_reads() {
    let root = Path::new(r"D:\repo");
    let full = git_full_path(root, "crates/bt-app/src/main.rs");
    assert!(full.starts_with(root));
    assert!(
        full.ends_with("main.rs"),
        "the file is still the file: {full:?}"
    );
    assert_eq!(
        full.components().count(),
        root.components().count() + 4,
        "and every folder git named is a folder of the path: {full:?}"
    );
}
