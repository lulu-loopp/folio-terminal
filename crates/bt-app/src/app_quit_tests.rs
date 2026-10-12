//! **The crate root: the quit.** Tests of items `main.rs` owns about the quit
//! transaction's surfaces — the cancelled quit's card (T-QUIT-DOCUMENTS) — written in the
//! crate root's scope (`use super::*`).

use super::*;

/// RED (T-QUIT-DOCUMENTS, revision (d)2) — **the cancelled quit's card names the files and
/// carries one choice.**
///
/// One Error card on the window: a plain statement — `settings.json could not be saved.`,
/// several files on one card, at most three named and then how many more — and one verb,
/// *Leave without this change*. The full list with each reason is `diagnostics.log`'s.
///
/// MUTATION: raise the card with no choice (`None` for the action in
/// `raise_cancelled_quit_card_on`) — the card offers nothing to press.
#[test]
fn the_cancelled_quits_card_names_the_files_and_carries_one_choice() {
    use i18n::Lang;
    let now = Instant::now();
    let cases: [(&[&'static str], &str); 4] = [
        (&["settings.json"], "settings.json could not be saved."),
        (
            &["settings.json", "profiles.json"],
            "settings.json and profiles.json could not be saved.",
        ),
        (
            &["settings.json", "keybindings.json", "profiles.json"],
            "settings.json, keybindings.json and profiles.json could not be saved.",
        ),
        (
            &[
                "settings.json",
                "keybindings.json",
                "profiles.json",
                "pins.json",
            ],
            "settings.json, keybindings.json, profiles.json and 1 more could not be saved.",
        ),
    ];
    for (files, sentence) in cases {
        let cancelled = quit::Cancelled::of(files);
        assert_eq!(
            i18n::quit_documents_not_saved_in(Lang::English, &cancelled.files()),
            sentence
        );
        let mut toasts = toast::ToastHost::default();
        let card = raise_cancelled_quit_card_on(&mut toasts, &cancelled, Motion::Full, now);
        let [raised] = toasts.toasts() else {
            panic!("one card: {:?}", toasts.toasts());
        };
        assert_eq!(raised.id(), card);
        assert_eq!(raised.kind(), toast::ToastKind::Error);
        assert_eq!(raised.body(), cancelled.sentence(), "the statement");
        assert_eq!(
            raised.action(),
            Some(i18n::Text::QuitLeaveWithoutChange.text()),
            "and its one choice"
        );
        assert_eq!(
            cancelled.lines().len(),
            files.len(),
            "every file, with why, goes to diagnostics.log"
        );
    }
    assert_eq!(
        i18n::Text::QuitLeaveWithoutChange.in_lang(Lang::English),
        "Leave without this change"
    );
}
