//! **`owner_door`, as the application drives it.** Tests whose first assertion is about
//! `owner_door`, written in the crate root's scope rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use crate::test_support::{found, reader_names, source};
use bt_source::{Pattern, View, needle};

/// RED (49) — **Only one function in bt-app calls Window::set_title.**
///
/// Five roads used to write the title straight to the OS — the drain, a tab
/// switch, a finished rename, a released synchronized update and a window's
/// birth — and a sixth written the same way would bring back the unthrottled
/// write this ticket removed. Read through `bt_source`, product files only.
///
/// **Since A1d the one function reaches it through its owner-thread door**: the winit call is
/// `owner_door::set_title`'s body, and that door is called from `flush_title` alone. The needle
/// reads three: the door's own name where it is declared, its winit call, and `flush_title`'s
/// call of it.
///
/// MUTATION: put a direct `self.window.window.set_title(&self.display_title())`
/// back in `Runtime::activate_tab` — red.
#[test]
fn only_one_function_in_bt_app_calls_window_set_title() {
    let calls =
        found(needle!(Pattern::call("set_title")), View::Identifiers).in_the_product(source());
    let mut readers = reader_names(&calls);
    readers.sort();
    assert_eq!(
        readers,
        vec!["flush_title".to_owned(), "set_title".to_owned()],
        "{}",
        calls.report(source())
    );
    assert_eq!(calls.len(), 3, "{}", calls.report(source()));
    assert_eq!(calls.outside_items(source()), 0);
}
