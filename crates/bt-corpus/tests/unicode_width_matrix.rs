use std::{cell::RefCell, num::NonZeroU32};

use bt_corpus::{Chunking, Corpus, CorpusEvent, EventKind};
use bt_term::TerminalAdapter;
use serde_json::Value;

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

#[test]
fn recorded_width_corpus_drives_legacy_and_mode_2027_oracles_byte_by_byte() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../../../corpus/cjk-width-cases.json")).unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let text = case["text"].as_str().unwrap();
        let clustered = case["expected_cells"].as_u64().unwrap() as u32;
        let legacy = case["expected_cells_bt_term"].as_u64().unwrap() as u32;
        let corpus = Corpus {
            initial_cols: 40,
            initial_rows: 4,
            conpty_source: None,
            events: vec![CorpusEvent {
                at_micros: 1,
                kind: EventKind::Output(text.as_bytes().to_vec()),
            }],
        };

        let mut legacy_term = TerminalAdapter::new(nz(40), nz(4));
        corpus
            .replay(
                Chunking::Pattern(vec![1, 2, 3]),
                |chunk| {
                    legacy_term.feed(chunk);
                },
                |_, _| unreachable!(),
            )
            .unwrap();
        assert_eq!(legacy_term.cursor().column, legacy, "legacy {id}");

        let mut clustered_term = TerminalAdapter::new(nz(40), nz(4));
        clustered_term.feed(b"\x1b[?2027h");
        corpus
            .replay(
                Chunking::Pattern(vec![1, 2, 3]),
                |chunk| {
                    clustered_term.feed(chunk);
                },
                |_, _| unreachable!(),
            )
            .unwrap();
        assert_eq!(clustered_term.cursor().column, clustered, "mode 2027 {id}");
    }
}

#[test]
fn replayed_resize_keeps_a_completed_cluster_in_one_lead_spacer_pair() {
    let terminal = RefCell::new(TerminalAdapter::new(nz(4), nz(3)));
    terminal.borrow_mut().feed(b"\x1b[?2027h");
    let corpus = Corpus {
        initial_cols: 4,
        initial_rows: 3,
        conpty_source: None,
        events: vec![
            CorpusEvent {
                at_micros: 1,
                kind: EventKind::Output("A👨‍👩‍👧‍👦BCDE".as_bytes().to_vec()),
            },
            CorpusEvent {
                at_micros: 2,
                kind: EventKind::Resize { cols: 8, rows: 3 },
            },
        ],
    };
    corpus
        .replay(
            Chunking::Pattern(vec![1, 4, 2]),
            |chunk| {
                terminal.borrow_mut().feed(chunk);
            },
            |columns, rows| {
                terminal
                    .borrow_mut()
                    .resize(NonZeroU32::from(columns), NonZeroU32::from(rows));
            },
        )
        .unwrap();

    let terminal = terminal.into_inner();
    let cluster = (0..3).find_map(|row| {
        let row = terminal.visible_row(row)?;
        let column = row.cells.iter().position(|cell| cell.text == "👨‍👩‍👧‍👦")?;
        Some((row, column))
    });
    let (row, column) = cluster.expect("cluster survives replayed resize");
    assert!(row.cells[column + 1].wide_spacer);
}
