//! **`webnav`, as the application drives it.** Tests whose first assertion is about
//! `webnav`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// RED (39) — **A `file:` address hands the file it names, whatever the file is, never the address.**
///
/// The road is a property of the address, not of page-ness. A `.html` on this disk keeps the path
/// road it has had since 2026-08-23 (`Runtime::open_local_path`, the files column's door), and so
/// does a `file:` address that is not a page — a picture, a program — because that door reads the
/// program list: a `file:///…/x.exe` must never leave as a bare address for the shell to run.
/// Real files, minted by the real `webnav::Mint::file`, read back by the real
/// `webnav::LocalFileUrl`.
///
/// MUTATION: in `preview_page_browser_hand_off`, answer a `web_url()` with
/// `preview_page_hand_off(source).map(PageHandOff::File)` instead of `LocalFileUrl::parse` — the
/// `.png` and the `.exe` hand nothing (or, with the `file:` fork removed too, the address).
#[test]
fn a_file_address_hands_the_file_it_names_whatever_the_file_is() {
    let dir = bt_testpath::temp_path("folio-t39-page-file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    for (name, bytes) in [
        ("a.html", &b"<h1>a</h1>"[..]),
        ("b.png", &b"PNG"[..]),
        ("x.exe", &b"MZ"[..]),
    ] {
        let file = dir.join(name);
        std::fs::write(&file, bytes).expect("a file on a disk");
        let canonical = std::fs::canonicalize(&file).expect("canonicalise it");
        let url = webnav::Mint::file(&canonical)
            .expect("a local path mints")
            .target()
            .expect("a mint names its URL")
            .to_owned();
        let Some(PageHandOff::File(path)) =
            preview_page_browser_hand_off(&preview::PreviewSource::Web(webnav::switcher_key(&url)))
        else {
            panic!("a file: address takes the file door, never the address: {url:?}");
        };
        assert_eq!(
            std::fs::canonicalize(&path).expect("the handed path names a file on the disk"),
            canonical,
            "the path handed over is the address's own file: {url:?}"
        );
    }
    // A local page shown as a file keeps the page answer, and a seat with no page has nothing.
    let page = std::fs::canonicalize(dir.join("a.html")).expect("the page");
    assert_eq!(
        preview_page_browser_hand_off(&preview::PreviewSource::file(&page)),
        Some(PageHandOff::File(page.clone()))
    );
    assert_eq!(
        preview_page_browser_hand_off(&preview::PreviewSource::file(dir.join("notes.md"))),
        None
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **A local page is kept by the same door that presses it** (§7.7 ⑤;
/// `plan.md` §3「钉不是授权」).
///
/// The bug this is the gate for: the switcher's pin did nothing at all on a
/// `file:///…/demo.html` row. W2 slice ⑤ gave the *press* a second door —
/// [`page_destination`], which takes a `file:` string back to the disk and
/// mints it again — and left the *pin* asking [`switcher_row_destination`],
/// which is `webnav::address_bar` and refuses `file:` from every door and
/// always will. So the two validations the design calls "the same door" were
/// two different doors, and the one that decides what reaches `pins.json`
/// was the one that cannot say yes to a local page. Nothing was drawn,
/// nothing was written, and the only report was an `eprintln!` nobody sees.
///
/// RED GATE: put [`switcher_row_destination`] back in
/// [`switcher_pin_is_allowed`] and the first assertion fails — the row the
/// user pressed is refused, exactly as it was on the real machine.
#[test]
fn a_local_page_is_kept_by_the_same_door_that_presses_it() {
    let dir = bt_testpath::temp_path("folio-switcher-pin");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let page = dir.join("sidebar-focus-demo.html");
    std::fs::write(&page, b"<h1>demo</h1>").expect("a page on a disk");
    let canonical = std::fs::canonicalize(&page).expect("canonicalise it");
    let url = webnav::Mint::file(&canonical)
        .expect("a local path mints")
        .target()
        .expect("a mint names its URL")
        .to_owned();

    assert!(
        switcher_pin_is_allowed(bt_persist::PinKind::Url, &url),
        "the row the user pressed reaches pins.json: {url}"
    );
    assert!(
        page_destination(&url).is_some(),
        "and it is the press door that said so, which is the whole claim"
    );
    // A page reached over the network is unchanged by any of this.
    assert!(switcher_pin_is_allowed(
        bt_persist::PinKind::Url,
        "https://example.com/report#ch3"
    ));

    // The three refusals that survive, each for its own reason. A `file:`
    // row is admitted by being taken back to the disk, so a string the disk
    // does not answer for is still refused — and now visibly.
    let missing = url.replace("sidebar-focus-demo", "never-existed");
    assert!(
        !switcher_pin_is_allowed(bt_persist::PinKind::Url, &missing),
        "a page whose file is not there is not kept: {missing}"
    );
    // A local file that is not a page could never have been a page row, and
    // the pin door asks the same question the lane fork asked to make one.
    let notes = dir.join("notes.md");
    std::fs::write(&notes, b"# notes\n").expect("a document on a disk");
    let notes_url =
        webnav::Mint::file(&std::fs::canonicalize(&notes).expect("canonicalise the document"))
            .expect("a local path mints")
            .target()
            .expect("a mint names its URL")
            .to_owned();
    assert!(
        !switcher_pin_is_allowed(bt_persist::PinKind::Url, &notes_url),
        "a document is not a page, however it is spelled: {notes_url}"
    );
    // And the file going away takes the pin's answer with it, which is the
    // press door's own answer for the same row.
    std::fs::remove_file(&page).expect("take the page away");
    assert!(!switcher_pin_is_allowed(bt_persist::PinKind::Url, &url));
    assert_eq!(page_destination(&url), None, "both doors, one answer");
    let _ = std::fs::remove_dir_all(&dir);
}

/// PIN (user ruling 2026-08-23) — **a page comes back from a session file as
/// a page, whichever of the two ways it was stored.**
///
/// The restore path is the one door `open_preview_source_on` is not on: a
/// tab's pool is seeded before the window that could host an engine exists,
/// so the turn-around happens where the pages are revived instead. Two
/// spellings reach it and both are the same file — the `Url` row every page
/// has written since W2 slice ⑤, and the `File` row a `.html` dropped on a
/// pane wrote while `.html` was still a document.
///
/// A real file in a real directory, because `canonicalize` is the step under
/// test — the row is a name and never a permission, so what authorises the
/// load is a mint made from the disk this instant.
///
/// RED GATE: drop the `source_opens_as_a_page` arm from [`revived_page_of`]
/// and the `File` half answers `None` — a restored tab comes back showing the
/// "no preview for this file type" card over a page it had rendered before
/// the restart.
#[test]
fn a_restored_page_comes_back_as_a_page_however_it_was_stored() {
    let dir = bt_testpath::temp_path("folio-page-revival");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let page = dir.join("report.html");
    std::fs::write(&page, b"<h1>x</h1>").expect("a page on a disk");
    let canonical = std::fs::canonicalize(&page).expect("canonicalise it");
    let minted = webnav::Mint::file(&canonical).expect("a local path mints");
    let url = minted.target().expect("a mint names its URL").to_owned();

    assert_eq!(
        revived_page_of(&preview::PreviewSource::Web(url.clone())),
        Some((url.clone(), minted.clone())),
        "a page stored as a page comes back as one, exactly as it always did"
    );
    assert_eq!(
        revived_page_of(&preview::PreviewSource::file(&page)),
        Some((url.clone(), minted)),
        "and a page stored as a file comes back as the same page, minted \
             from the same disk"
    );
    // A document is not a page and is revived by the head-read lane, so this
    // door answers nothing for it.
    let notes = dir.join("notes.md");
    std::fs::write(&notes, b"# notes\n").expect("a document on a disk");
    assert_eq!(revived_page_of(&preview::PreviewSource::file(&notes)), None);
    // And a row naming a file that is not there goes nowhere, which is the
    // answer §7.9 ⑤ already gives for a row this window would not navigate to.
    std::fs::remove_file(&page).expect("take the file away");
    assert_eq!(revived_page_of(&preview::PreviewSource::file(&page)), None);
    assert_eq!(revived_page_of(&preview::PreviewSource::Web(url)), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// PIN (W2 slice 5) - **a stored row naming a local page is taken back to
/// the disk, never trusted as a string.**
///
/// [`page_destination`] is what a switcher row, a `session.json` line and a
/// hand-edited pin all leave by, and its `file:` arm does the same three
/// steps the files column does: decode to a path, canonicalise it against
/// the disk, mint from *that*. So what authorises the load is a mint the
/// host made this instant, and the row contributes a name and nothing more.
///
/// The three refusals below are the shape of that: a path that is not there
/// has nothing to canonicalise, a `..` in the string never reaches the disk
/// at all, and a percent escape this door does not write is a URL somebody
/// else built.
///
/// A real file in a real directory, because `canonicalize` is the step under
/// test and there is no way to ask it about a disk that is not there.
///
/// RED GATE: return `Some((target.to_owned(), Mint::Nothing))` for a `file:`
/// string - the round trip below still passes and every refusal fails, which
/// is the whole difference between naming a file and being allowed to load
/// one.
#[test]
fn a_stored_local_page_is_minted_from_the_disk_and_not_from_the_row() {
    let dir = bt_testpath::temp_path("folio-slice5-page-destination");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let page = dir.join("report.html");
    std::fs::write(&page, b"<h1>x</h1>").expect("a page on a disk");
    let canonical = std::fs::canonicalize(&page).expect("canonicalise it");
    let minted = webnav::Mint::file(&canonical).expect("a local path mints");
    let url = minted
        .target()
        .expect("a file mint names its URL")
        .to_owned();

    let (destination, carried) = page_destination(&url).expect("a row naming a file that is there");
    assert_eq!(destination, url, "the same page, minted again");
    assert_eq!(carried, minted, "and the mint travels with it");

    // The fragment a local report's table of contents uses is the page's own
    // business and survives the trip.
    let with_fragment = format!("{url}#chapter-3");
    assert_eq!(
        page_destination(&with_fragment).map(|(url, _)| url),
        Some(with_fragment),
        "the page answers for its own fragment"
    );

    std::fs::remove_file(&page).expect("take the file away");
    assert_eq!(
        page_destination(&url),
        None,
        "a row naming a file that is not there goes nowhere"
    );
    for hostile in [
        "file:///C:/site/../../Windows/win.ini",
        "file:///C:/site/%2e%2e/secret.html",
        "file://server/share/page.html",
        "file:///",
    ] {
        assert!(
            page_destination(hostile).is_none(),
            "not a string this door minted: {hostile}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
