use std::cmp::Ordering;
use std::num::{NonZeroU32, NonZeroUsize};

use bt_doc::{
    AnchorError, Bias, ContentAnchor, DecorationIntent, DetectionRevision, GridPoint, LayoutKey,
    ScreenId, compare_anchors,
};
use bt_term::DualPlaneSession;

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn nz_size(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

#[test]
fn g3_live_to_staging_to_history_migrations_are_atomic_transactions() {
    let mut session = DualPlaneSession::new(nz(4), nz(2));
    let removed = session.register_live_anchor(
        ScreenId::Primary,
        GridPoint { row: 0, column: 2 },
        Bias::After,
    );
    let nonzero_survivor = session.register_live_anchor(
        ScreenId::Primary,
        GridPoint { row: 1, column: 1 },
        Bias::Before,
    );

    session.feed(b"abcde\r\n").unwrap();
    assert!(matches!(
        session.anchor(removed).unwrap(),
        ContentAnchor::Staging { .. }
    ));
    assert!(matches!(
        session.anchor(nonzero_survivor).unwrap(),
        ContentAnchor::Live {
            point: GridPoint { row: 0, column: 1 },
            ..
        }
    ));

    session.feed(b"\x1b[2;1H\r\n").unwrap();
    assert!(matches!(
        session.anchor(removed).unwrap(),
        ContentAnchor::History { .. }
    ));
    assert_eq!(session.document().entries().len(), 1);
}

#[test]
fn g3_ed3_degrades_deleted_history_anchor_to_live_origin() {
    let mut session = DualPlaneSession::new(nz(4), nz(2));
    let anchor = session.register_live_anchor(
        ScreenId::Primary,
        GridPoint { row: 0, column: 1 },
        Bias::After,
    );
    session.feed(b"abcde\r\n\x1b[2;1H\r\n").unwrap();
    assert!(matches!(
        session.anchor(anchor).unwrap(),
        ContentAnchor::History { .. }
    ));
    session.feed(b"\x1b[3J").unwrap();
    assert!(matches!(
        session.anchor(anchor).unwrap(),
        ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 0, column: 0 },
            bias: Bias::Before,
            ..
        }
    ));
}

#[test]
fn g3_quota_eviction_degrades_to_the_next_surviving_history_entry() {
    let mut session = DualPlaneSession::with_frozen_quota(nz(8), nz(2), nz_size(2));
    let anchor = session.register_live_anchor(
        ScreenId::Primary,
        GridPoint { row: 0, column: 1 },
        Bias::After,
    );
    let _nonzero_fixture = session.register_live_anchor(
        ScreenId::Primary,
        GridPoint { row: 1, column: 0 },
        Bias::Before,
    );
    session.feed(b"one\r\ntwo\r\nthree").unwrap();
    let first = *session.document().entries().first_key_value().unwrap().0;
    assert!(matches!(
        session.anchor(anchor).unwrap(),
        ContentAnchor::History { id, .. } if *id == first
    ));

    session.feed(b"\r\nfour\r\nfive").unwrap();
    let successor = *session.document().entries().first_key_value().unwrap().0;
    assert_ne!(successor, first);
    assert!(matches!(
        session.anchor(anchor).unwrap(),
        ContentAnchor::History { id, bias: Bias::Before, .. } if *id == successor
    ));
}

#[test]
fn g3_primary_and_alternate_namespaces_cannot_be_compared() {
    let mut session = DualPlaneSession::new(nz(8), nz(3));
    let primary = session.register_live_anchor(
        ScreenId::Primary,
        GridPoint { row: 2, column: 0 },
        Bias::Before,
    );
    let alternate = session.register_live_anchor(
        ScreenId::Alternate,
        GridPoint { row: 1, column: 0 },
        Bias::Before,
    );
    assert_eq!(
        compare_anchors(
            session.anchor(primary).unwrap(),
            session.anchor(alternate).unwrap()
        ),
        Err(AnchorError::IsolatedScreen)
    );
    assert_eq!(
        compare_anchors(
            session.anchor(primary).unwrap(),
            session.anchor(primary).unwrap()
        ),
        Ok(Ordering::Equal)
    );
}

#[test]
fn g3_stale_worker_result_is_rejected_after_each_public_version_boundary() {
    for boundary in 0..4 {
        let mut session = DualPlaneSession::new(nz(16), nz(2));
        session.feed(b"$$x$$\r\nnext\r\ntail").unwrap();
        let task = session.take_worker_task().unwrap();
        match boundary {
            0 => session.redetect(DetectionRevision(2)),
            1 => session.set_layout_key(LayoutKey {
                width_cells: nz(8),
                ..session.layout_key()
            }),
            2 => session.bump_view_generation(),
            3 => session.feed(b"\x1b[3J").unwrap(),
            _ => unreachable!(),
        }
        assert!(!session.complete_worker_task(task));
        assert_eq!(session.stale_results(), 1);
    }
}

#[test]
fn g3_redetection_rebuilds_document_intent_before_projection_consumes_revision() {
    let mut session = DualPlaneSession::new(nz(16), nz(2));
    session.feed(b"$$x$$\r\nnext\r\ntail").unwrap();
    session.redetect(DetectionRevision(7));
    assert!(session.document().entries().values().any(|entry| matches!(
        entry.decoration,
        DecorationIntent::Math {
            detection_revision: DetectionRevision(7),
            ..
        }
    )));
    let projection = session.new_projection(session.layout_key());
    assert_eq!(projection.detection_revision(), DetectionRevision(7));
}
