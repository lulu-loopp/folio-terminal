//! The copy-back card's own tests (0.4.10 plan row 68d, R6-R8): one card for every offered
//! document, its keys, and what *Bring back* does to the layout — on a headless window over
//! scratch folders of its own.

use std::path::PathBuf;
use std::sync::Arc;

use winit::keyboard::{Key, NamedKey};

use crate::carry_pass::CarrySnapshot;
use crate::copy_back::{CopyBackAnswer, CopyOffered, OfferUp};
use crate::headless::{HeadlessApplication, HeadlessFolio};
use crate::rejected_copies::KeptDocument;
use crate::test_support::{leaf_saying, saved_tab, tab_holding};

/// An offer of `document`'s copy named `copy`, holding `bytes`, one version above the start's.
fn offered(document: KeptDocument, copy: &str, bytes: Vec<u8>, version: u32) -> CopyOffered {
    CopyOffered {
        document,
        copy: copy.to_owned(),
        path: PathBuf::from(copy),
        bytes: Arc::from(bytes),
        copy_version: version,
        file_version: version - 1,
    }
}

/// A folder row opened at `at`, ISO 8601 UTC.
fn folder_row(path: &str, at: &str) -> bt_persist::RecentFolderV1 {
    bt_persist::RecentFolderV1 {
        path: path.to_owned(),
        opened_at: at.to_owned(),
    }
}

/// RED (0.4.10 plan row 68d, R7) — **bringing back the layout opens the copy's windows beside the
/// open ones**: *Bring back* (`Tab`, `Tab`, `Enter`) queues each saved window to open as a saved
/// window, the open window stands as it was, and the recent folders are merged newest first, one
/// row per folder, the newer moment winning.
///
/// MUTATION: the copy's layout replaces the open one — the open window is asked to close
/// (`plant-68d-layout-closes-the-open-windows.py`).
#[test]
fn bringing_back_the_layout_opens_its_windows_beside_the_open_ones() {
    let mut folio = HeadlessFolio::with_tabs(
        "copy-back-layout",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    );
    let id = folio.window.window.id;
    let older = crate::seed::parse_iso8601_utc("2026-11-01T08:00:00Z").unwrap();
    folio.app.recent_folders.record("C:/here", older);
    let windows: Vec<bt_persist::SessionWindowV1> = ["C:/a", "C:/b"]
        .into_iter()
        .map(|cwd| bt_persist::SessionWindowV1 {
            tabs: vec![saved_tab("pwsh", cwd, None, true)],
            ..bt_persist::SessionWindowV1::default()
        })
        .collect();
    let saved = bt_persist::SessionV1 {
        windows: windows.clone(),
        recent_folders: vec![
            folder_row("C:/here", "2026-11-14T10:00:00Z"),
            folder_row("C:/there", "2026-11-14T09:00:00Z"),
        ],
        ..bt_persist::SessionV1::default()
    };
    folio.app.copy_back = Some(OfferUp {
        window: id,
        offer: vec![offered(
            KeptDocument::Session,
            "session.json.rejected-20261114-000000",
            bt_persist::serialize_session(&saved).unwrap(),
            bt_persist::SESSION_SCHEMA_VERSION,
        )],
        card: crate::update_card::Card::default(),
    });
    let tabs_before = folio.window.tabs.len();
    {
        let mut runtime = folio.runtime();
        assert!(runtime.copy_back_card_is_up());
        for key in [NamedKey::Tab, NamedKey::Tab, NamedKey::Enter] {
            runtime
                .press_copy_back_card_key(&Key::Named(key))
                .expect("a key on the card is answered");
        }
    }
    assert!(folio.app.copy_back.is_none(), "the card went");
    let queued: Vec<bt_persist::SessionWindowV1> = folio
        .app
        .pending_new_windows
        .iter()
        .map(|plan| *plan.saved.clone().expect("a saved window"))
        .collect();
    assert_eq!(queued, windows, "the copy's windows, to open beside");
    assert!(folio.window.leaving.is_none(), "the open window stands");
    assert_eq!(
        folio.window.window_close_requested, None,
        "and is not asked to close"
    );
    assert_eq!(folio.window.tabs.len(), tabs_before, "with its tabs");
    let rows: Vec<(String, String)> = folio
        .app
        .recent_folders
        .to_persisted()
        .into_iter()
        .map(|row| (row.path, row.opened_at))
        .collect();
    assert_eq!(
        rows,
        [
            ("C:/here".to_owned(), "2026-11-14T10:00:00Z".to_owned()),
            ("C:/there".to_owned(), "2026-11-14T09:00:00Z".to_owned()),
        ]
    );
}

/// RED (row 68d, R6) — **the copy-back card asks once for every offered document**: one card
/// in the window the round was adopted in, naming both documents; `Escape` puts it away and
/// writes nothing; and a later round offering the same copies asks nothing more at this start.
///
/// MUTATION: the card offers its first document alone (`plant-68d-one-card-per-document.py`).
#[test]
fn the_copy_back_card_asks_once_for_every_document() {
    let mut application = HeadlessApplication::of(HeadlessFolio::with_tabs(
        "copy-back-once",
        vec![tab_holding(leaf_saying("PS> "))],
        0,
    ));
    let settings = offered(
        KeptDocument::Settings,
        "settings.json.rejected-20261114-000001",
        bt_persist::serialize_settings(&bt_persist::SettingsV1::default()).unwrap(),
        bt_persist::SETTINGS_SCHEMA_VERSION,
    );
    let pins = offered(
        KeptDocument::Pins,
        "pins.json.rejected-20261114-000002",
        bt_persist::serialize_pins(&bt_persist::PinsV1::default()).unwrap(),
        bt_persist::PINS_SCHEMA_VERSION + 1,
    );
    let round = || CarrySnapshot {
        copy_back: Some(CopyBackAnswer {
            notices: Vec::new(),
            offer: vec![settings.clone(), pins.clone()],
        }),
        ..CarrySnapshot::default()
    };
    application
        .folio
        .adopt_carried(round())
        .expect("the round is adopted");
    let id = application.id;
    let app = application.folio.app.as_ref().expect("the application");
    let up = app.copy_back.as_ref().expect("one card");
    assert_eq!(up.window, id);
    assert_eq!(up.offer, [settings.clone(), pins.clone()]);
    assert_eq!(
        up.paint().heading.as_deref(),
        Some("Bring back your newer settings and pins?")
    );
    let writes = (
        app.settings_store.write_generation(),
        app.pins_store.write_generation(),
    );

    application
        .runtime()
        .press_copy_back_card_key(&Key::Named(NamedKey::Escape))
        .expect("Escape is answered");
    let app = application.folio.app.as_ref().expect("the application");
    assert!(app.copy_back.is_none(), "Escape put the card away");
    assert!(app.copy_back_owed.is_empty(), "and brought nothing back");
    assert_eq!(
        (
            app.settings_store.write_generation(),
            app.pins_store.write_generation()
        ),
        writes,
        "and wrote nothing"
    );

    application
        .folio
        .adopt_carried(round())
        .expect("a later round is adopted");
    assert!(
        application
            .folio
            .app
            .as_ref()
            .expect("the application")
            .copy_back
            .is_none(),
        "the same copies are not asked again at this start"
    );
}
