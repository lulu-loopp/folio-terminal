//! **`attention_wire`, as the application drives it.** Tests whose first assertion is about
//! `attention_wire`, written in the crate root's scope rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use crate::test_support::{
    calls_of, found, found_in_package, in_product, method_body, package_item_body,
};
use bt_source::{ItemQuery, Pattern, Scope, View, needle};

/// **Cancelling a composition goes through one door** (§7.1.5a″).
///
/// Two claims, and the pins are what keep them true: the Win32 notification
/// is named in exactly one place in this workspace, and this window reaches
/// it through exactly one function of its own — which is also the function
/// that clears everything this window was drawing from the composition. A
/// second spelling of either is a candidate list left on the glass with its
/// letters gone, or letters cleared with the list still up.
///
/// MUTATION: clear `window.preedit` at a second site without the platform
/// call and the last assertion goes red.
#[test]
fn cancelling_a_composition_goes_through_one_door() {
    assert_eq!(
        found_in_package(
            "bt-platform",
            needle!(Pattern::identifier("NI_COMPOSITIONSTR")),
            View::Identifiers,
            Scope::Module("crate".to_owned()),
        )
        .len(),
        2,
        "named where it is imported and where it is called, and nowhere else",
    );
    // **`cancel_composition` is declared three times in that crate** — once in
    // each of `windows_impl`, `macos_ime` and `portable_ime` — so the query
    // says which arm it means. The text needle this replaces said the same
    // thing by accident, by spelling the Windows arm's signature.
    assert!(
        package_item_body(
            "bt-platform",
            &ItemQuery::function("cancel_composition").in_module("crate::windows_impl"),
        )
        .contains("NI_COMPOSITIONSTR"),
        "and the place it is called is the door",
    );
    // The needle no longer has to be spelled in two pieces: a name written
    // inside a string literal is one token and not a path, so this line cannot
    // be one of the occurrences it is counting.
    assert_eq!(
        in_product(&found(
            needle!(Pattern::path("bt_platform::cancel_composition")),
            View::Identifiers,
        )),
        1,
        "and it is reached from exactly one place in this window",
    );
    let door = method_body("Runtime", "cancel_composition");
    for (cleared, what) in [
        ("bt_platform::cancel_composition", "the method's own state"),
        ("self.window.preedit = None", "the letters"),
        ("ime_cursor.reset()", "the rectangle the list hung from"),
        (
            "destroy_ime_caret(\"cancel_composition\")",
            "the caret Pinyin follows",
        ),
        ("set_preedit(\"\")", "the field's own copy of them"),
    ] {
        assert!(door.contains(cleared), "the one door lets go of {what}");
    }
    assert!(
        !door.contains("set_ime_allowed"),
        "and it does not re-associate the window's input context to do it",
    );
    // The one place that notices is the tail of the pass, which is where
    // every way of moving the keyboard has already happened.
    assert_eq!(
        in_product(&calls_of("Runtime", "settle_composition_owner")),
        1,
        "one watcher, and no list of the ways a field can go away",
    );
}
