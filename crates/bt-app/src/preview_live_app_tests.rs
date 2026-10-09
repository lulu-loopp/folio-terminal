//! **`preview_live`, as the application drives it.** Tests whose first assertion is about
//! `preview_live`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// **The candidate list hangs at the composition's own caret** (user report
/// 2026-09-12).
///
/// A list offering to finish `nikan` that stood at the byte the `n` went in
/// front of would sit over the letters it is offering to replace, which is
/// §7.1.3u's complaint one surface along: the box and the bar are one
/// derivation, and while a composition is in flight that derivation is the
/// composition's.
///
/// MUTATION: hang the box off `prose.caret(caret)` while composing and the
/// first assertion goes red — the two x's are a whole pre-edit apart.
#[test]
fn the_candidate_box_sits_at_the_composition_caret_not_the_block_caret() {
    // One row of `我nikan看` as a body face lays it out: sixteen pixels an
    // ideograph, eight a latin letter, and a seam in front of every cluster.
    let seams: Vec<preview_live::ProseSeam> = [
        (0, 0.0),
        (3, 16.0),
        (4, 24.0),
        (5, 32.0),
        (6, 40.0),
        (7, 48.0),
        (8, 56.0),
        (11, 72.0),
    ]
    .into_iter()
    .map(|(offset, x)| preview_live::ProseSeam { offset, x })
    .collect();
    let cut = preview_live::split_prose_row(
        100.0,
        20.0,
        &seams,
        0,
        Some(preview_live::ProseSplice {
            at: 3,
            len: 5,
            caret: 2,
        }),
    );
    let rows = preview_live::ProseRows {
        index: Some(0),
        rows: vec![cut.row.clone()],
        composition: Some(preview_live::ProseComposition {
            rows: cut.composition.into_iter().collect(),
            caret: cut.caret,
        }),
    };
    assert_eq!(
        cut.caret.map(|rect| rect[0]),
        Some(32.0),
        "the box hangs two letters into the composition, where the method put its caret",
    );
    assert_eq!(
        rows.caret(3).map(|rect| rect[0]),
        Some(56.0),
        "while the block's own caret is the byte after the letters, a whole \
             pre-edit away",
    );
    assert_eq!(
        cut.composition,
        Some([16.0, 100.0, 56.0, 120.0]),
        "and the rule under the composition spans exactly the letters being typed",
    );
    // The file's own seams are the file's: what is in the paragraph and not
    // in the file is gone from them, and everything after the composition is
    // back where the file has it.
    assert_eq!(
        cut.row
            .seams
            .iter()
            .map(|seam| (seam.offset, seam.x))
            .collect::<Vec<_>>(),
        vec![(0, 0.0), (3, 56.0), (6, 72.0)],
    );
}
