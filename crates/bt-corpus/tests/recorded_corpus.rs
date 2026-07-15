use std::{cell::RefCell, fs::File, num::NonZeroU32, path::PathBuf};

use bt_corpus::{Chunking, Corpus};
use bt_term::TerminalAdapter;

fn corpus_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus")
        .join(name)
}

fn replay(name: &str, chunking: Chunking) -> (Vec<String>, Vec<String>, (usize, usize)) {
    let corpus = Corpus::read_from(File::open(corpus_path(name)).unwrap()).unwrap();
    let terminal = RefCell::new(TerminalAdapter::new(
        NonZeroU32::new(u32::from(corpus.initial_cols)).unwrap(),
        NonZeroU32::new(u32::from(corpus.initial_rows)).unwrap(),
    ));
    corpus
        .replay(
            chunking,
            |bytes| {
                terminal.borrow_mut().feed(bytes);
            },
            |cols, rows| {
                terminal
                    .borrow_mut()
                    .resize(NonZeroU32::from(cols), NonZeroU32::from(rows));
            },
        )
        .unwrap();
    let terminal = terminal.into_inner();
    let visible = terminal.visible_text();
    let frozen = terminal
        .transcript()
        .frozen()
        .iter()
        .map(|line| line.text.clone())
        .collect();
    let dimensions = terminal.dimensions();
    (
        visible,
        frozen,
        (dimensions.0.get() as usize, dimensions.1.get() as usize),
    )
}

#[test]
fn recorded_corpora_are_invariant_under_arbitrary_byte_chunking() {
    for name in [
        "pwsh-daily.btcr",
        "shell-dollars.btcr",
        "tui-redraw.btcr",
        "editor-alt-screen.btcr",
        "resize-sequence.btcr",
        "claude-code-session.btcr",
    ] {
        let recorded = replay(name, Chunking::Recorded);
        let fuzzed = replay(name, Chunking::Pattern(vec![1, 7, 2, 31, 3]));
        assert_eq!(recorded, fuzzed, "chunking changed replay for {name}");
    }
}

#[test]
fn corpus_exercises_dollars_alt_screen_and_resize_markers() {
    let (_, dollars, _) = replay("shell-dollars.btcr", Chunking::Fixed(5));
    assert!(dollars.iter().any(|line| line.contains("literal=$$")));

    let (editor_visible, editor_history, _) = replay("editor-alt-screen.btcr", Chunking::Fixed(3));
    assert!(
        editor_visible
            .iter()
            .any(|line| line.contains("editor exited"))
    );
    assert!(!editor_history.iter().any(|line| line.contains("INSERT")));

    let corpus =
        Corpus::read_from(File::open(corpus_path("resize-sequence.btcr")).unwrap()).unwrap();
    assert_eq!(
        corpus
            .events
            .iter()
            .filter(|event| matches!(event.kind, bt_corpus::EventKind::Resize { .. }))
            .count(),
        4
    );
    assert_eq!(
        replay("resize-sequence.btcr", Chunking::Fixed(11)).2,
        (90, 22)
    );
}

#[test]
fn claude_fixture_is_a_real_interactive_tui_recording() {
    let corpus =
        Corpus::read_from(File::open(corpus_path("claude-code-session.btcr")).unwrap()).unwrap();
    let output = corpus
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            bt_corpus::EventKind::Output(bytes) => Some(bytes.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    assert!(
        output
            .windows(b"Claude Code v2.1.210".len())
            .any(|w| w == b"Claude Code v2.1.210")
    );
    assert!(
        output
            .windows(b"Now revise steps 4 and 9".len())
            .any(|w| w == b"Now revise steps 4 and 9")
    );
    assert!(
        output
            .windows(b"Searching for 2 patterns".len())
            .any(|w| w == b"Searching for 2 patterns")
    );
    assert!(output.windows(8).any(|w| w == b"\x1b[?1049h"));
    assert_eq!(
        corpus
            .events
            .iter()
            .filter(|event| matches!(event.kind, bt_corpus::EventKind::Resize { .. }))
            .count(),
        2
    );
}
