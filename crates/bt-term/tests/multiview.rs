use std::num::NonZeroU32;

use bt_doc::{Bias, ContentAnchor, GridPoint, LayoutKey, ScreenId};
use bt_term::{DualPlaneSession, SPIKE_CELL_HEIGHT_SUBPIXELS};
use bt_transcript::GraphemeOffset;
use bt_viewport::{ScrollAnchor, ViewSelection};

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn key(width_cells: u32) -> LayoutKey {
    LayoutKey {
        width_cells: nz(width_cells),
        dpi_milli: nz(1000),
        font_rev: 1,
        theme_rev: 1,
        lang_rev: 0,
        profile_rev: 0,
        line_wrapping: true,
    }
}

#[test]
fn g2_two_widths_project_height_selection_and_scroll_independently() {
    let mut session = DualPlaneSession::new(nz(16), nz(2));
    session.feed(b"abcdefgh\r\nnext\r\ntail").unwrap();
    let entry = session.document().entries().first_key_value().unwrap().1;
    let history = ContentAnchor::History {
        id: entry.line.id,
        offset: GraphemeOffset(6),
        bias: Bias::Before,
        generation: entry.line.source_generation,
    };
    let nonzero_live_id = session.register_live_anchor(
        ScreenId::Primary,
        GridPoint { row: 1, column: 0 },
        Bias::After,
    );
    let nonzero_live = session.anchor(nonzero_live_id).unwrap().clone();

    let mut narrow = session.new_projection(key(4));
    let mut wide = session.new_projection(key(8));
    narrow.set_selection(Some(ViewSelection {
        start: history.clone(),
        end: nonzero_live.clone(),
    }));
    narrow.set_scroll_anchor(Some(ScrollAnchor {
        source: history.clone(),
        local_offset: 7,
    }));
    wide.set_scroll_anchor(Some(ScrollAnchor {
        source: history.clone(),
        local_offset: 19,
    }));

    assert_eq!(
        narrow.heights().total(),
        2 * SPIKE_CELL_HEIGHT_SUBPIXELS.get()
    );
    assert_eq!(wide.heights().total(), SPIKE_CELL_HEIGHT_SUBPIXELS.get());
    assert_eq!(
        narrow.anchor_y(session.document(), &history).unwrap(),
        SPIKE_CELL_HEIGHT_SUBPIXELS.get()
    );
    assert_eq!(wide.anchor_y(session.document(), &history).unwrap(), 0);
    assert_eq!(
        narrow.anchor_y(session.document(), &nonzero_live).unwrap(),
        3 * SPIKE_CELL_HEIGHT_SUBPIXELS.get()
    );
    assert_eq!(
        wide.anchor_y(session.document(), &nonzero_live).unwrap(),
        2 * SPIKE_CELL_HEIGHT_SUBPIXELS.get()
    );
    assert!(narrow.selection_y(session.document()).unwrap().is_some());
    assert!(wide.selection_y(session.document()).unwrap().is_none());
    assert_eq!(
        narrow.scroll_y(session.document()).unwrap().unwrap(),
        SPIKE_CELL_HEIGHT_SUBPIXELS.get() + 7
    );
    assert_eq!(wide.scroll_y(session.document()).unwrap().unwrap(), 19);

    let narrow_misses = narrow.cache_misses();
    wide.relayout(key(6), session.document());
    assert_eq!(narrow.cache_misses(), narrow_misses);
    assert_ne!(wide.layout_key(), narrow.layout_key());
}
