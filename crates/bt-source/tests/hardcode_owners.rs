//! Structural ownership pins for T-HARDCODE-047 slice 1.
//!
//! These ask the `bt-source` index about symbols, calls and decoded literals.
//! They deliberately do not read a named source file or use source text as a
//! behavior oracle: each assertion names the production item that may own or
//! read a moved fact, and a former production site becoming an owner again is
//! the failure.

use bt_source::{Found, Index, ItemQuery, Pattern, Scope, Search, View, needle};

fn product(index: &'static Index, pattern: Pattern, view: View) -> Found {
    index
        .search(&Search::new(needle!(pattern), view))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(index)
}

fn inside(index: &'static Index, item: ItemQuery, pattern: Pattern, view: View) -> Found {
    index
        .search(&Search::new(needle!(pattern), view).in_scope(Scope::Item(item)))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(index)
}

fn owner_names(found: &Found, index: &Index) -> Vec<(String, usize)> {
    found
        .owners(index)
        .into_iter()
        .map(|(owner, count)| (owner.to_string(), count))
        .collect()
}

/// MUTATION: restore `bt_platform::order_cjk_families` inside
/// `with_automatic_cjk`; the forbidden call is then owned by that former site.
#[test]
fn plt_1_language_order_is_owned_only_by_cjk_publication() {
    let app = Index::of_package("bt-app");
    let old_sort = product(app, Pattern::call("order_cjk_families"), View::Identifiers);
    assert_eq!(old_sort.len(), 0, "{}", old_sort.report(app));

    let language_order = product(app, Pattern::call("order_for_language"), View::Identifiers);
    assert_eq!(
        owner_names(&language_order, app),
        [("crate::settings::CjkFamilySlot::publish".to_owned(), 2)],
        "{}",
        language_order.report(app)
    );
    assert_eq!(language_order.outside_items(app), 0);
}

/// MUTATION: restore `name_is_writable` and its Windows character class in
/// `bt-app`; either the symbol or the former grammar spelling becomes a second
/// owner, while the one platform judge remains the only reader below it.
#[test]
fn prv_1_file_name_grammar_has_one_platform_owner() {
    let app = Index::of_package("bt-app");
    for (pattern, view) in [
        (Pattern::identifier("name_is_writable"), View::Identifiers),
        (
            Pattern::text(r#"'\\' | '/' | ':' | '*' | '?'"#),
            View::CodeKeepingLiterals,
        ),
    ] {
        let former = product(app, pattern, view);
        assert_eq!(former.len(), 0, "{}", former.report(app));
    }

    let readers = product(
        app,
        Pattern::path("bt_platform::judge_file_name"),
        View::Identifiers,
    );
    assert_eq!(
        owner_names(&readers, app),
        [("crate::files::judge_new_name_on".to_owned(), 1)],
        "{}",
        readers.report(app)
    );
}

/// MUTATION: restore the `WINDOWS_POWERSHELL_ID` lookup in
/// `Registry::set_hidden_on`; the forbidden identifier appears in the method.
#[test]
fn set_1_hidden_floor_reads_the_platform_fallback_owner() {
    let app = Index::of_package("bt-app");
    let method = ItemQuery::method("Registry", "set_hidden_on").in_module("crate::profiles");
    let old_floor = inside(
        app,
        method.clone(),
        Pattern::identifier("WINDOWS_POWERSHELL_ID"),
        View::Identifiers,
    );
    assert_eq!(old_floor.len(), 0, "{}", old_floor.report(app));

    let owner_call = inside(
        app,
        method,
        Pattern::call("fallback_profile_in_on"),
        View::Identifiers,
    );
    assert_eq!(owner_call.len(), 1, "{}", owner_call.report(app));
}

/// MUTATION: restore either local `"wsl.exe"` comparison in `derived_paths`;
/// the launcher spelling returns outside the shell-family descriptor.
#[test]
fn set_4_shell_decisions_flow_through_the_family_descriptor() {
    let app = Index::of_package("bt-app");

    for reader in [
        "derive_integration",
        "derive_grammar_on",
        "login_flag",
        "derived_paths",
    ] {
        let query = ItemQuery::function(reader).in_module("crate::profiles");
        for spelling in [
            "bash",
            "zsh",
            "fish",
            "nu",
            "pwsh",
            "powershell",
            "cmd",
            "wsl",
            "wsl.exe",
            "csh",
            "tcsh",
            "-l",
        ] {
            let copied_fact = inside(
                app,
                query.clone(),
                Pattern::text(spelling),
                View::LiteralValues,
            );
            assert_eq!(
                copied_fact.len(),
                0,
                "{reader} copied {spelling:?} from the family row:\n{}",
                copied_fact.report(app)
            );
        }
        let family = inside(
            app,
            query.clone(),
            Pattern::call("shell_family"),
            View::Identifiers,
        );
        assert_eq!(family.len(), 1, "{reader}:\n{}", family.report(app));
        if reader == "derived_paths" {
            let suffix_rule = inside(app, query, Pattern::call("ends_with"), View::Identifiers);
            assert_eq!(suffix_rule.len(), 0, "{}", suffix_rule.report(app));
        }
    }
}

/// MUTATION: restore either former product-chip constant; its decoded English
/// literal is again present in `bt-detect` or `bt-term` product code.
#[test]
fn trm_1_overlay_matching_owns_no_product_wording() {
    for package in ["bt-detect", "bt-term"] {
        let index = Index::of_package(package);
        let chip = product(
            index,
            Pattern::text("Jump to bottom (ctrl+End)"),
            View::LiteralValues,
        );
        assert_eq!(chip.len(), 0, "{}", chip.report(index));
    }
}

/// MUTATION: restore `FeedSeam` and the byte-window scanners in session code;
/// the old type returns and the two semantic consumers start scanning windows.
#[test]
fn trm_2_session_consumes_parser_controls_instead_of_scanning_bytes() {
    let term = Index::of_package("bt-term");
    let seam = product(term, Pattern::identifier("FeedSeam"), View::Identifiers);
    assert_eq!(seam.len(), 0, "{}", seam.report(term));

    for function in [
        "contains_clear_home_snapshot_boundary",
        "cursor_visibility_toggles",
    ] {
        let windows = inside(
            term,
            ItemQuery::function(function).in_module("crate::session"),
            Pattern::call("windows"),
            View::Identifiers,
        );
        assert_eq!(windows.len(), 0, "{function}:\n{}", windows.report(term));
    }
}

/// MUTATION: restore any of the former standalone Windows CJK arrays; the old
/// font-list symbol or an extra copy of the representative family/file literal
/// appears outside the one relation.
#[test]
fn rnd_2_windows_cjk_facts_are_rows_of_one_table() {
    let render = Index::of_package("bt-render");
    let old_files = product(
        render,
        Pattern::identifier("CJK_FALLBACK_FONT_FILES"),
        View::Identifiers,
    );
    assert_eq!(old_files.len(), 0, "{}", old_files.report(render));

    for (literal, expected) in [("Microsoft YaHei UI", 1), ("msyh.ttc", 2)] {
        let occurrences = product(render, Pattern::text(literal), View::LiteralValues);
        assert_eq!(
            occurrences.len(),
            expected,
            "{literal}:\n{}",
            occurrences.report(render)
        );
    }
}

/// MUTATION: copy the percent-decoder body back into
/// `bt_viewport::file_uri_printed_form`; that former caller gains `to_digit`
/// calls (and normally loses its shared-owner call), so this pin fails.
#[test]
fn file_uri_syntax_and_percent_decoding_have_one_owner() {
    let transcript = Index::of_package("bt-transcript");
    let component_readers = product(
        transcript,
        Pattern::call("decode_uri_component_bytes"),
        View::Identifiers,
    );
    assert_eq!(
        owner_names(&component_readers, transcript),
        [
            ("crate::paths::decode_file_uri_address".to_owned(), 2,),
            ("crate::paths::decode_uri_component_bytes".to_owned(), 1,),
            ("crate::paths::decode_uri_component_utf8".to_owned(), 1,),
        ],
        "{}",
        component_readers.report(transcript)
    );

    let callers = [
        (
            "bt-platform",
            ItemQuery::function("path_from_file_url").in_module("crate::app_delegate"),
        ),
        (
            "bt-platform",
            ItemQuery::function("file_uri_to_path_on").in_module("crate"),
        ),
        (
            "bt-viewport",
            ItemQuery::function("file_uri_printed_form").in_module("crate"),
        ),
    ];
    for (package, caller) in callers {
        let index = Index::of_package(package);
        let shared = inside(
            index,
            caller.clone(),
            Pattern::call("decode_file_uri_address"),
            View::Identifiers,
        );
        assert_eq!(shared.len(), 1, "{caller}:\n{}", shared.report(index));
        let private_loop = inside(index, caller, Pattern::call("to_digit"), View::Identifiers);
        assert_eq!(
            private_loop.len(),
            0,
            "a former caller owns a percent-decoding loop:\n{}",
            private_loop.report(index)
        );
    }

    let viewport = Index::of_package("bt-viewport");
    let old_helper = product(
        viewport,
        Pattern::identifier("percent_decoded"),
        View::Identifiers,
    );
    assert_eq!(old_helper.len(), 0, "{}", old_helper.report(viewport));
}
