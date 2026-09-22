use super::*;

fn native_ime() -> NativeFacts {
    NativeFacts {
        imm_is_ime: Some(true),
        context: Some(true),
        open: Some(true),
        conversion: Some(1),
        ..NativeFacts::default()
    }
}

fn focused() -> Report {
    let mut report = Report::default();
    report.focus(true, 10);
    report
}

fn keys(report: &mut Report, terminal: bool) -> bool {
    (0..3).any(|_| report.key(terminal, true))
}

#[test]
fn ime_self_report_fires_once_per_focus_and_rearms() {
    let mut report = focused();
    assert!(!report.key(true, true));
    assert!(!report.key(true, true));
    assert!(report.key(true, true));
    assert!(report.confirm(native_ime()));
    assert!(!report.confirm(native_ime()));
    report.focus(true, 15); // A duplicate announcement is the same focus epoch.
    assert!(!keys(&mut report, true));
    report.focus(false, 20);
    assert!(!keys(&mut report, true));
    report.focus(true, 30);
    assert!(keys(&mut report, true));
    assert!(report.confirm(native_ime()));
}

#[test]
fn ime_self_report_rejects_plain_layout_english_and_unknown_modes() {
    for facts in [
        NativeFacts {
            imm_is_ime: Some(false),
            ..native_ime()
        },
        NativeFacts {
            open: Some(false),
            ..native_ime()
        },
        NativeFacts {
            conversion: Some(0),
            ..native_ime()
        },
        NativeFacts {
            conversion: Some(8),
            ..native_ime()
        }, // Full shape, still alphanumeric.
        NativeFacts {
            conversion: None,
            ..native_ime()
        },
        NativeFacts {
            context: Some(false),
            open: None,
            conversion: None,
            ..native_ime()
        },
        NativeFacts::default(),
    ] {
        let mut report = focused();
        assert!(keys(&mut report, true));
        assert!(!report.confirm(facts));
    }
    let mut report = focused();
    assert!(keys(&mut report, true));
    assert!(report.confirm(NativeFacts {
        imm_is_ime: Some(false),
        tsf_profile_type: Some(1),
        ..native_ime()
    }));
}

#[test]
fn ime_self_report_any_ime_event_suppresses_until_next_focus() {
    for kind in [
        ImeKind::Enabled,
        ImeKind::Preedit,
        ImeKind::Commit,
        ImeKind::Disabled,
    ] {
        let mut report = focused();
        report.ime(kind, 12);
        assert!(!keys(&mut report, true));
        report.focus(false, 20);
        report.focus(true, 30);
        assert!(keys(&mut report, true));
        assert!(report.confirm(native_ime()));
    }
}

#[test]
fn ime_self_report_requires_consecutive_terminal_keys() {
    let mut report = focused();
    assert!(!keys(&mut report, false)); // Settings or preview.
    assert!(!report.key(true, true));
    assert!(!report.key(false, true));
    assert!(!report.key(true, true));
    assert!(!report.key(true, false));
    assert!(!report.key(true, true));
    assert!(!report.key(true, true));
    assert!(report.key(true, true));
    assert!(report.confirm(native_ime()));
}

#[test]
fn ime_self_report_format_has_facts_but_never_typed_text() {
    let secret = "privateTypedSentinel";
    assert!(printable_latin(Some(secret)));
    let mut report = focused();
    report.created(0);
    report.allowed(true, 1);
    report.shown(2);
    report.first_key(13);
    assert!(keys(&mut report, true));
    assert!(report.confirm(native_ime()));
    let line = report.line("plain-text", 14, true, false, native_ime());
    assert!(line.starts_with("Folio: keys are arriving as plain text"));
    for field in [
        "allowed=",
        "enabled_since_focus=false",
        "created=",
        "shown=",
        "first_focus=",
        "first_enabled=",
        "first_key=",
        "native=",
        "web_host=false",
    ] {
        assert!(line.contains(field), "missing {field}");
    }
    assert!(!line.contains(secret));
    assert!(!line.contains('\n'));
    assert!(!printable_latin(Some("\r")));
    assert!(!printable_latin(None));
}

// ── what this module asks the crate ────────────────────────
//
// **P3's deletion commit for this batch** (`docs/plans/bt-app-split-prep.md`
// §6.3, and §6.0 rule 3). The commit before this one took every reading
// twice — once from the window's file, included as text, once from `bt-source`
// — and asserted the two answered the same; this one removes the older of the
// two, because two implementations of one judgement do not vouch for each
// other (`docs/CONVENTIONS.md` §十 rule 4).
//
// **The pattern is `main.rs::pty_drain_budget_tests`' and is not re-derived**;
// that module's header carries the six points behind `source_index`,
// `item_body` and `method_body`. This module lives in another file than
// `main.rs`, which changes neither: `needle!` records *this* file's site and
// the index is the package's either way (§2.6).
//
// Two readings change shape rather than merely moving.
//
// * The two **arm cuts** — the focus arm and the composing prologue — took the
//   first occurrence of a text in the whole file. The item that owns the arm is
//   named now and the same cut is taken inside its body.
//   `WindowEvent::Focused(true) => {` is spelled in `main.rs`'s own test
//   modules too, so the old reading was right only by the order that file
//   happens to be written in.
// * The **constructor loop** walked every `.create_window(attributes)` in the
//   file and read everything written after each. The two constructors are
//   `Runtime::create` and `Runtime::open_window`, so they are named, and the
//   order is asked inside each one's own body.

/// **This crate, indexed once per process** — the workspace read, this
/// package's own `src/` declared as the universe and lowered, on the first ask
/// of the process, behind one call (`bt_source::Index::of_package`).
///
/// The package is named here and nowhere else in the module.
fn source_index() -> &'static bt_source::Index {
    bt_source::Index::of_package("bt-app")
}

/// The body of `owner::name`, braces included — the identity of §2.4 rather
/// than a line of `main.rs`.
fn item_body(query: &bt_source::ItemQuery) -> &'static str {
    source_index()
        .body_of(query)
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// The body of one inherent method of `owner`.
fn method_body(owner: &str, name: &str) -> &'static str {
    item_body(&bt_source::ItemQuery::method(owner, name))
}

/// The body of one method of a trait's implementation — the window's own entry
/// points, which are not inherent methods of anything.
fn trait_method_body(owner: &str, trait_name: &str, name: &str) -> &'static str {
    item_body(&bt_source::ItemQuery::method(owner, name).of_trait(trait_name))
}

/// One search over the whole package, refusing loudly rather than
/// answering a smaller question.
fn found(needle: bt_source::Needle, view: bt_source::View) -> bt_source::Found {
    source_index()
        .search(&bt_source::Search::new(needle, view))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// How many of these occurrences stand in a file a product build compiles.
///
/// File-grained, and deliberately so: §2.3 computes product reachability
/// per *declaration path to a file*, and an inline `#[cfg(test)] mod`
/// inside a product file is not a file.
fn in_product(found: &bt_source::Found) -> usize {
    found
        .occurrences()
        .iter()
        .filter(|occurrence| {
            source_index()
                .file_at(occurrence.span.start())
                .is_some_and(bt_source::FileRecord::permits_product)
        })
        .count()
}

/// The package's product count of one raw needle — the view `include_str!`
/// handed this module.
fn in_product_raw(needle: bt_source::Needle) -> usize {
    in_product(&found(needle, bt_source::View::Raw))
}

#[test]
fn ime_self_report_startup_and_focus_source_pin() {
    assert_eq!(
        in_product_raw(bt_source::needle!(bt_source::Pattern::text(
            "ime_report.created(ime_report::now_ms());"
        ))),
        2
    );
    assert_eq!(
        in_product_raw(bt_source::needle!(bt_source::Pattern::text(
            "ime_report.allowed(true, ime_report::now_ms());"
        ))),
        2
    );
    assert!(
        in_product_raw(bt_source::needle!(bt_source::Pattern::text(
            "self.window.ime_report.shown(ime_report::now_ms());"
        ))) > 0
    );
    let focus_arm = trait_method_body("FolioApp", "ApplicationHandler", "window_event")
        .split("WindowEvent::Focused(true) => {")
        .nth(1)
        .expect("the window's event road carries the focus arm");
    assert!(
        focus_arm
            .split("runtime.set_cursor_focus(true")
            .next()
            .unwrap()
            .contains("runtime.observe_ime_focus(true)")
    );
    assert!(
        in_product_raw(bt_source::needle!(bt_source::Pattern::text(
            "runtime.observe_ime_key(&event, is_synthetic);"
        ))) > 0
    );
    assert!(
        method_body("Runtime", "ime_input")
            .split("let composing =")
            .next()
            .unwrap()
            .contains("self.window.ime_report.ime(")
    );
    assert!(
        in_product_raw(bt_source::needle!(bt_source::Pattern::text(
            "runtime.service_ime_report(now)"
        ))) > 0
    );
    assert!(
        in_product_raw(bt_source::needle!(bt_source::Pattern::text(
            "earliest_deadline([wake_deadline, ime_deadline])"
        ))) > 0
    );
    for owner in ["create", "open_window"] {
        let constructor = method_body("Runtime", owner)
            .split(".create_window(attributes)")
            .nth(1)
            .unwrap_or_else(|| panic!("`Runtime::{owner}` constructs a window"));
        let created = constructor.find("ime_report.created(").unwrap();
        let allowed_call = constructor.find("window.set_ime_allowed(true)").unwrap();
        let allowed_record = constructor.find("ime_report.allowed(true,").unwrap();
        assert!(created < allowed_call && allowed_call < allowed_record);
    }
}

#[test]
fn ime_self_report_rechecks_mode_only_at_the_threshold_of_a_streak() {
    let mut report = focused();
    assert!(keys(&mut report, true));
    assert!(!report.confirm(NativeFacts {
        conversion: Some(0),
        ..native_ime()
    }));
    for _ in 0..100 {
        assert!(!report.key(true, true));
    }
    assert!(!report.key(true, false));
    assert!(keys(&mut report, true));
    assert!(report.confirm(native_ime()));
}

#[test]
fn ime_self_report_stamps_preserve_order_even_at_the_same_injected_time() {
    let mut report = Report::default();
    report.created(0);
    report.allowed(true, 0);
    report.shown(0);
    report.focus(true, 0);
    report.ime(ImeKind::Enabled, 0);
    report.first_key(0);
    let stamps = [
        report.created,
        report.allowed.map(|(_, s)| s),
        report.shown,
        report.first_focus,
        report.first_enabled,
        report.first_key,
    ];
    for (i, stamp) in stamps.into_iter().enumerate() {
        assert_eq!(stamp.unwrap().order as usize, i + 1);
        assert_eq!(stamp.unwrap().ms, 0);
    }
    report.focus(false, 1);
    assert!(report.enabled_since_focus); // The blur snapshot retains that epoch's evidence.
    report.focus(true, 2);
    assert!(!report.enabled_since_focus);
    assert_eq!(report.first_enabled.unwrap().order, 5);
}

#[test]
fn ime_self_report_reads_native_facts_once_per_interval_within_a_focus() {
    let mut report = focused();
    // Two hundred words typed in nine seconds: one reading, not two hundred.
    let mut readings = 0;
    for word in 0..200u64 {
        assert!(keys(&mut report, true));
        if report.may_probe(word * 45) {
            readings += 1;
            assert!(!report.confirm(NativeFacts {
                conversion: Some(0),
                ..native_ime()
            }));
        }
        assert!(!report.key(true, false));
    }
    assert_eq!(readings, 1);
    // The interval over, the next streak is read again, and can still report.
    assert!(keys(&mut report, true));
    assert!(report.may_probe(PROBE_MIN_INTERVAL_MS + 1));
    assert!(report.confirm(native_ime()));
    // A new focus starts a new budget.
    report.focus(false, 0);
    report.focus(true, 0);
    assert!(report.may_probe(1));
}

#[test]
fn ime_self_report_names_a_restart_inside_a_live_composition_and_an_unpaired_end() {
    let mut report = focused();
    // An ordinary composition says nothing.
    assert_eq!(report.pairing(ImeKind::Enabled, 0), None);
    assert_eq!(report.pairing(ImeKind::Preedit, 5), None);
    assert_eq!(report.pairing(ImeKind::Preedit, 0), None);
    assert_eq!(report.pairing(ImeKind::Commit, 0), None);
    assert_eq!(report.pairing(ImeKind::Disabled, 0), None);
    // The incident's shape: a second start while five bytes are live, then the
    // teardown, then an end nobody opened.
    assert_eq!(report.pairing(ImeKind::Enabled, 0), None);
    assert_eq!(report.pairing(ImeKind::Preedit, 5), None);
    let restart = report
        .pairing(ImeKind::Enabled, 0)
        .expect("a restart is named");
    assert!(restart.contains("shape=restarted-inside-a-live-composition"));
    assert!(restart.contains("live_preedit_bytes=5"));
    assert_eq!(report.pairing(ImeKind::Preedit, 1), None);
    assert_eq!(report.pairing(ImeKind::Preedit, 0), None);
    assert_eq!(report.pairing(ImeKind::Commit, 0), None);
    assert_eq!(report.pairing(ImeKind::Disabled, 0), None);
    let unpaired = report
        .pairing(ImeKind::Disabled, 0)
        .expect("an unpaired end is named");
    assert!(unpaired.contains("shape=ended-without-a-start"));
    // Bounded for the life of the window.
    let mut lines = 2;
    for _ in 0..100 {
        lines += usize::from(report.pairing(ImeKind::Disabled, 0).is_some());
    }
    assert_eq!(lines, usize::from(PAIRING_LINES_MAX));
}
