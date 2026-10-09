//! **The crate root: language.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;

/// PIN — **the file's three answers and the table's two languages line up,
/// and `System` is the only one that asks the machine.**
///
/// The mapping used to be written out inside `Runtime::new`; it is a free
/// function now because the Language row calls it again on every press, and
/// a second spelling of it is how a row and a settings file come to disagree
/// about what `System` means.
#[test]
fn a_stored_language_resolves_to_the_column_it_names() {
    use bt_persist::LanguageV1;
    assert_eq!(
        super::resolved_language(LanguageV1::English),
        i18n::Lang::English
    );
    assert_eq!(
        super::resolved_language(LanguageV1::Chinese),
        i18n::Lang::Chinese
    );
    assert_eq!(
        super::resolved_language(LanguageV1::System),
        i18n::resolve(i18n::LanguageMode::System, &bt_platform::os_ui_language()),
        "`System` is the machine's answer and nothing else"
    );
}
