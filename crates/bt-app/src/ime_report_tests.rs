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

#[test]
fn ime_self_report_startup_and_focus_source_pin() {
    let source = include_str!("main.rs");
    assert_eq!(
        source
            .matches("ime_report.created(ime_report::now_ms());")
            .count(),
        2
    );
    assert_eq!(
        source
            .matches("ime_report.allowed(true, ime_report::now_ms());")
            .count(),
        2
    );
    assert!(source.contains("self.window.ime_report.shown(ime_report::now_ms());"));
    let focus = source
        .split("WindowEvent::Focused(true) => {")
        .nth(1)
        .unwrap();
    assert!(
        focus
            .split("runtime.set_cursor_focus(true")
            .next()
            .unwrap()
            .contains("runtime.observe_ime_focus(true)")
    );
    assert!(source.contains("runtime.observe_ime_key(&event, is_synthetic);"));
    let ime = source
        .split("fn ime_input(&mut self, event: Ime)")
        .nth(1)
        .unwrap();
    assert!(
        ime.split("let composing =")
            .next()
            .unwrap()
            .contains("self.window.ime_report.ime(")
    );
    assert!(source.contains("runtime.service_ime_report(now)"));
    assert!(source.contains("earliest_deadline([wake_deadline, ime_deadline])"));
    for constructor in source.split(".create_window(attributes)").skip(1) {
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
