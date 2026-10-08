//! **`settings`, as the application drives it.** Tests whose first assertion is about
//! `settings`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// PIN (GitHub issue #3) — **opening the dialog asks the machine for
/// nothing.**
///
/// An outside user reported the window freezing for seconds when the gear is
/// clicked. The cause was one line of this file: `settings_values` asks
/// `settings::family_index` which family is ticked, on the press that opens
/// the dialog, and the list behind it had just been marked stale by the same
/// press — so every open walked DirectWrite's whole system font collection on
/// this thread, opening a font face per family to name its files.
///
/// What this pins is the negative, which is the only half a counter can
/// state and the only half that was ever in doubt: **reading the list a
/// frame draws performs no walk**. That covers the press, the hover, the hit
/// test and the draw, because all four reach the list through exactly these
/// two functions.
///
/// A delta and not a total, because the counter belongs to the thread and
/// the test harness may run another test on it first.
///
/// Red gate: put the enumeration back behind `monospace_families` — the
/// revision-keyed `MonospaceFamilySlot::get` this replaced — and the count
/// moves on the first line. See `settings::MonospaceFamilySlot` for the
/// shape that keeps it still, and the three tests beside it for the halves a
/// counter cannot state.
#[test]
fn opening_the_dialog_asks_the_machine_for_no_fonts() {
    let before = settings::monospace_scans();
    let list = settings::monospace_families();
    // Every road the dialog takes to the list, in the order the press takes
    // them: which row is ticked, how many rows there are, and what each one
    // reads.
    let ticked = settings::family_index(bt_platform::DEFAULT_MONOSPACE_FAMILY);
    let cjk = settings::cjk_families();
    let cjk_ticked = settings::cjk_family_index("");
    let drawn: Vec<&str> = list.iter().map(|family| family.name.as_str()).collect();
    let again = settings::monospace_families();
    assert_eq!(
        settings::monospace_scans(),
        before,
        "the dialog read both family lists {} times and walked no font \
             collection to do it (ticked rows {ticked}/{cjk_ticked}, {} families drawn)",
        drawn.len() + cjk.len() + 5,
        drawn.len(),
    );
    assert_eq!(
        list.as_ptr(),
        again.as_ptr(),
        "and two reads are one list, so a page redrawn on hover cannot be \
             drawn from two"
    );
}

/// The gear no longer *is* the theme switch — it opens the surface the
/// switch lives on, and nothing about a caption button decides a colour any
/// more. The theme now comes from a press on a picker item, which
/// `settings::theme_requested` answers and `settings.rs` pins.
///
/// Red gate: the previous version of this test asserted the gear returned
/// the opposite theme. That function is gone, and this one fails the moment
/// something starts deciding a theme from a `ChromeTarget` again.
#[test]
fn the_gear_opens_the_settings_surface_rather_than_deciding_a_theme() {
    let mut panel = settings::SettingsPanel::default();
    let rows = settings::visible_rows(seats::TabLayoutMode::Horizontal);
    panel.toggle(settings::SettingsContent {
        rows: &rows,
        shortcuts: &[],
        profiles: &[],
        scheme_files: &[],
        advanced: settings::AdvancedOpen::default(),
        advanced_reveal: None,
        editor: None,
        values: Box::leak(Box::new(settings::SettingsValues::sample())),
    });
    assert!(panel.is_open(), "the gear's verb is 'open the dialog'");
    assert_eq!(
        settings::theme_requested(settings::SettingsTarget::Close),
        None,
        "nothing but a picker item asks for a theme"
    );
    assert_eq!(
        settings::theme_requested(settings::SettingsTarget::Choice(
            settings::SettingsRow::Theme,
            0
        )),
        Some(ThemeModeV1::Light),
        "the mock-up's picker opens with Light (2500)"
    );
}
