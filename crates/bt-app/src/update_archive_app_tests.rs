//! **`update_archive`, as the application drives it.** Tests whose first assertion is about
//! `update_archive`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::method_body;

/// PIN (user ruling 2026-08-23) — **the glance card says what the row opens
/// as.**
///
/// The defect this closes was visible without running anything: a `.pdf` row
/// drew "No preview — binary or unrecognized type." under a resting pointer
/// and opened a rendered page under a double click. The card and the row are
/// two readings of one gesture, so they read it through one function
/// ([`preview_open_lane`]) and the card can no longer contradict the door.
///
/// **The card no longer stops at saying it** (user ruling 2026-08-25): the
/// page class splits on whether the file's own bytes are readable, and each
/// half shows what it has. `.html` shows its markup — it is text, and it goes
/// down the very lane every text file goes down — while `.pdf` shows the two
/// facts a binary container can still state. The chip and the foot are
/// untouched by both, so the row still says `web` and still says how to open
/// it.
///
/// RED GATE: delete the `Web` arm of [`peek_body_kind`] — `.pdf` falls to the
/// refusal arm (its buffer has no reader) and says "no preview" over a row a
/// double click renders. Point both halves of
/// [`preview::path_page_glance`] at `Source` and a `.pdf` card asks the
/// document pipeline for a body no reader in this window can build.
#[test]
fn the_glance_card_says_what_the_row_opens_as() {
    let kind = |name: &str, refused: bool| {
        let path = PathBuf::from(format!(r"D:\site\{name}"));
        peek_body_kind(preview::preview_ftype(name), Some(&path), refused, false)
    };
    // **A page whose bytes are text shows them.** One lane, the document's,
    // and no branch of its own below this line.
    for name in ["index.html", "index.htm", "INDEX.HTM"] {
        assert_eq!(
            kind(name, false),
            PeekBodyKind::Document,
            "a page made of text shows its source: {name}"
        );
        // And when that read came back refused — a binary body under an
        // `.html` name, a file that went away — the refusal is what the card
        // owes, exactly as for any other document.
        assert_eq!(kind(name, true), PeekBodyKind::Refused, "{name}");
    }
    // **A page made of nothing this window reads states its facts instead**,
    // and states them whatever the buffer behind it holds: there is no read
    // to be refused, because nothing is read.
    for name in ["report.pdf", "REPORT.PDF"] {
        assert_eq!(kind(name, false), PeekBodyKind::Facts, "{name}");
        assert_eq!(kind(name, true), PeekBodyKind::Facts, "{name}");
    }
    // The regression half — every other class draws exactly what it drew.
    assert_eq!(kind("notes.md", false), PeekBodyKind::Document);
    assert_eq!(kind("main.rs", false), PeekBodyKind::Document);
    assert_eq!(kind("notes.md", true), PeekBodyKind::Refused);
    assert_eq!(kind("a.exe", false), PeekBodyKind::Refused);
    assert_eq!(kind("shot.png", false), PeekBodyKind::Picture);
    assert_eq!(kind("index.htmlx", false), PeekBodyKind::Refused);
    assert_eq!(kind("report.html.txt", false), PeekBodyKind::Document);
    // A page on a share is not a page: the mint refuses it, so the card is
    // the refusal the seat would have shown.
    assert_eq!(
        peek_body_kind(
            preview::PreviewFtype::Web,
            Some(Path::new(r"\\server\share\index.html")),
            true,
            false,
        ),
        PeekBodyKind::Refused
    );
    // A composed document has no path, so it is drawn as the document it is
    // however its name is spelled.
    assert_eq!(
        peek_body_kind(preview::PreviewFtype::Web, None, false, false),
        PeekBodyKind::Document
    );
    assert_eq!(
        peek_body_kind(preview::PreviewFtype::Image, None, false, false),
        PeekBodyKind::Refused,
        "and a picture with no file is still a picture nothing can decode"
    );

    // **And the glance really reads with the glance's buffer.** The ladder
    // above says a `.html` row shows a document; what makes a document
    // *arrive* is the buffer the card is armed with, and a card armed with a
    // pane's buffer would ask no disk at all and sit empty for ever — the
    // one failure the ladder cannot see. A fact about this file, so it is
    // read off this file, exactly as the pool's own door is two tests up.
    let arming = method_body("Runtime", "mature_file_peek");
    assert!(
        arming.contains("preview::PreviewBuffer::glancing("),
        "the glance arms itself with a pane's buffer, so a page's source is \
             never read and the card stays empty:\n{arming}"
    );
}
