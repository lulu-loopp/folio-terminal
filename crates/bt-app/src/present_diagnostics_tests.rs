use super::*;

#[test]
fn freshness_is_debt_gated_and_decades_are_values() {
    for owed in [false, true] {
        for shown in [false, true] {
            for minimized in [false, true] {
                assert_eq!(
                    decision(owed, shown, minimized, 1_000_000, 0, false),
                    (owed && shown && !minimized).then_some(Line::Stale)
                );
            }
        }
    }
    assert_eq!(decision(true, true, false, 999_999, 0, false), None);
    assert_eq!(decision(true, true, false, 9_999_999, 1, false), None);
    assert_eq!(
        decision(true, true, false, 10_000_000, 1, false),
        Some(Line::Stale)
    );
    assert_eq!(
        decision(true, true, false, 100_000_000, 2, false),
        Some(Line::Stale)
    );
    assert_eq!(
        decision(false, true, false, 100_000_000, 2, true),
        Some(Line::Landed)
    );
    assert_eq!(decision(false, true, false, 100_000_000, 0, true), None);
    assert_eq!(decision(false, true, false, u64::MAX, 2, false), None);
}

#[test]
fn freshness_replacement_does_not_restart_age_and_landing_closes_once() {
    let mut state = State::default();
    state.observe(
        true,
        10,
        Progress {
            events: 2,
            turns: 3,
        },
    );
    state.observe(
        true,
        500_000,
        Progress {
            events: 8,
            turns: 9,
        },
    );
    assert_eq!(state.pending_since, Some(10));
    assert_eq!(
        state.check(true, true, false, 1_000_010, false),
        Some(Line::Stale)
    );
    assert_eq!(state.check(true, true, false, 1_000_011, false), None);
    assert_eq!(
        state.check(true, true, false, 10_000_010, false),
        Some(Line::Stale)
    );
    assert_eq!(
        state.check(false, true, false, 10_000_011, true),
        Some(Line::Landed)
    );
    state.observe(false, 10_000_011, Progress::default());
    assert_eq!(state.check(false, true, false, 10_000_012, true), None);
}

#[test]
fn attempt_format_round_trips_numbers_and_unknown_native_facts() {
    let mut attempt = Attempt::new(7, 3, 11, bt_render::FrameSource::Resize, true, false, 20);
    attempt.outcome = Outcome::NotVisible;
    attempt.timings = [1, 2, 3, 4, 5, 6];
    let text = attempt.line(
        bt_platform::NativePresentFacts::default(),
        true,
        (false, None),
        bt_render::PresentConfiguration {
            generation: 3,
            mode: bt_render::PresentMode::Mailbox,
            latency: 1,
            wait: "Wait",
        },
        40,
        50,
    );
    let fields: std::collections::BTreeMap<_, _> = text
        .split_whitespace()
        .skip(2)
        .map(|field| field.split_once('=').unwrap())
        .collect();
    for field in [
        "native_iconic",
        "native_cloaked",
        "native_client",
        "native_style_visible",
        "attention_age_us",
    ] {
        assert_eq!(fields[field], "unknown");
    }
    for (field, value) in [
        ("win", 7),
        ("gen", 3),
        ("seq", 11),
        ("configure_us", 1),
        ("acquire_us", 2),
        ("encode_us", 3),
        ("submit_us", 4),
        ("present_us", 5),
        ("commit_us", 6),
        ("since_last_present_us", 40),
        ("pending_age_us", 50),
    ] {
        assert_eq!(fields[field].parse::<u64>().unwrap(), value);
    }
    assert_eq!(fields["outcome"], "not_visible");
    assert_eq!(
        fields.len(),
        24,
        "the trace vocabulary is closed and carries no user text"
    );
    let number = |name: &str| fields[name].parse::<u64>().unwrap();
    let mut decoded = Attempt::new(
        number("win"),
        number("gen"),
        number("seq"),
        match fields["src"] {
            "Resize" => bt_render::FrameSource::Resize,
            other => panic!("unexpected source {other}"),
        },
        number("retained") != 0,
        false,
        0,
    );
    decoded.outcome = match fields["outcome"] {
        "not_visible" => Outcome::NotVisible,
        other => panic!("unexpected outcome {other}"),
    };
    decoded.timings = [
        "configure_us",
        "acquire_us",
        "encode_us",
        "submit_us",
        "present_us",
        "commit_us",
    ]
    .map(number);
    let reconstructed = decoded.line(
        bt_platform::NativePresentFacts::default(),
        number("folio_shown") != 0,
        (number("attention_exposed") != 0, None),
        bt_render::PresentConfiguration {
            generation: number("gen"),
            mode: match fields["mode"] {
                "Mailbox" => bt_render::PresentMode::Mailbox,
                other => panic!("unexpected mode {other}"),
            },
            latency: number("latency") as u32,
            wait: match fields["wait"] {
                "Wait" => "Wait",
                other => panic!("unexpected wait {other}"),
            },
        },
        number("since_last_present_us"),
        number("pending_age_us"),
    );
    assert_eq!(reconstructed, text);
}

#[test]
fn every_present_caller_closes_its_record_outside_all_outcome_arms() {
    let source = include_str!("main.rs");
    for name in ["present_retained_picture", "redraw"] {
        let start = source
            .find(&format!("    fn {name}(&mut self) -> Result<()> {{"))
            .unwrap();
        let tail = &source[start + 5..];
        let end = [tail.find("\n    ///"), tail.find("\n    fn ")]
            .into_iter()
            .flatten()
            .min()
            .unwrap();
        let body = &source[start..start + 5 + end];
        assert!(body.contains("begin_present_attempt("), "{name}");
        assert!(
            body.contains("let result = (|| {") || body.contains("let result = hang_watch::during"),
            "{name}"
        );
        assert!(
            body.contains("finish_present_attempt(attempt, &result)"),
            "{name}"
        );
        assert!(
            body.rfind("finish_present_attempt").unwrap()
                > body.rfind("Self::present_seats_and_commit").unwrap()
        );
    }
    let start = source.find("    fn present_seats_and_commit(").unwrap();
    let body = &source[start..source[start..].find("\n    ///").unwrap() + start];
    for outcome in [
        "Unchanged",
        "Presented",
        "WithoutText",
        "Skipped",
        "NotVisible",
        "Reconfigure",
    ] {
        assert!(body.contains(&format!("Outcome::{outcome}")), "{outcome}");
    }
    assert!(
        source.contains("let owed = owed || self.picture_is_owed();"),
        "an empty redraw must not restart standing debt"
    );
    let module = include_str!("present_diagnostics.rs");
    for forbidden in [
        ".title()",
        ".display()",
        "error.to_string()",
        "{error}",
        "{error:#}",
    ] {
        assert!(
            !module.contains(forbidden),
            "diagnostics must accept no user text: {forbidden}"
        );
    }
}

#[test]
fn phase_timings_are_exclusive_values_and_close_an_error_phase() {
    let mut attempt = Attempt::new(7, 3, 11, bt_render::FrameSource::Resize, false, false, 0);
    attempt.phase_at(Some(2), 10);
    attempt.phase_at(Some(0), 20);
    attempt.phase_at(Some(1), 35);
    attempt.phase_at(Some(2), 60);
    attempt.phase_at(Some(3), 90);
    attempt.phase_at(Some(4), 95);
    attempt.phase_at(Some(5), 200);
    attempt.phase_at(None, 230);
    assert_eq!(attempt.timings, [15, 25, 40, 5, 105, 30]);
    attempt.phase_at(Some(0), 250);
    attempt.phase_at(None, 300); // configure returns an error
    assert_eq!(attempt.timings[0], 65);
}

#[test]
fn freshness_format_counts_attempts_and_actual_dispatch_progress() {
    let mut state = State::default();
    state.observe(
        true,
        100,
        Progress {
            events: 3,
            turns: 4,
        },
    );
    state.last_present = Some(0);
    state.last_landed = (2, 9);
    let mut attempt = Attempt::new(7, 3, 11, bt_render::FrameSource::Resize, false, false, 0);
    attempt.outcome = Outcome::NotVisible;
    state.attempted(&attempt);
    state.attempted(&attempt);
    attempt.outcome = Outcome::FailedCommit;
    state.attempted(&attempt);
    let line = state.line(
        7,
        1_000_100,
        Progress {
            events: 8,
            turns: 10,
        },
        Line::Stale,
    );
    assert_eq!(
        line,
        "Folio: window 7 has shown no new picture for 1000 ms — last present 1000 ms ago (gen 3, seq 11, outcome failed:commit); 3 attempts since, 2 of them not_visible; the window thread dispatched 5 events and turned 6 times in that span; last_landed_gen=2 last_landed_seq=9"
    );
    let parsed: Vec<u64> = line
        .split_whitespace()
        .filter_map(|word| word.trim_end_matches(',').parse().ok())
        .collect();
    assert_eq!(parsed, [7, 1000, 1000, 3, 11, 3, 2, 5, 6]);
    assert!(
        state
            .line(7, 1_000_100, Progress::default(), Line::Landed)
            .contains("; a picture landed;")
    );
    assert_eq!(
        native_fields(bt_platform::NativePresentFacts::default()),
        "native_iconic=unknown native_cloaked=unknown native_client=unknown native_style_visible=unknown"
    );
}

#[test]
fn existing_hidden_callers_keep_the_same_fused_value() {
    let source = include_str!("main.rs");
    let body = source
        .split("fn window_is_hidden(window: &Window) -> bool {")
        .nth(1)
        .unwrap()
        .split("\n}")
        .next()
        .unwrap();
    assert!(body.contains("return false;"));
    assert!(body.contains(
        "bt_platform::is_window_minimized(native) || bt_platform::is_window_cloaked(native)"
    ));
    for (minimized, cloaked, expected) in [
        (false, false, false),
        (false, true, true),
        (true, false, true),
        (true, true, true),
    ] {
        assert_eq!(minimized || cloaked, expected);
    }
    assert!(source.contains("let hidden = window_is_hidden(window);"));
}

#[test]
fn attempt_generation_is_carried_by_configure_and_native_reads_are_line_gated() {
    let source = include_str!("main.rs");
    let start = source.find("    fn finish_present_attempt(").unwrap();
    let body = &source[start
        ..source[start..]
            .find("    fn check_picture_freshness(")
            .unwrap()
            + start];
    assert!(
        body.find("if self.app.trace_perf").unwrap() < body.find("native_present_facts").unwrap()
    );
    let start = source.find("    fn check_picture_freshness(").unwrap();
    let body = &source[start..source[start..].find("    ///").unwrap() + start];
    assert!(body.find("if let Some(line)").unwrap() < body.find("native_present_facts").unwrap());
    let render = include_str!("../../bt-render/src/lib.rs");
    assert!(render.contains("phase(PresentPhase::SurfaceConfigure(self.surface_generation + 1));"));
    assert_eq!(render.matches("let present_wait = descriptor").count(), 3);
    assert!(render.contains("self.present_wait = present_wait;"));
}

#[test]
fn a_resting_window_stays_silent_for_every_attempt_outcome_and_hidden_debt_survives() {
    for index in 0..9 {
        let mut state = State::default();
        let mut attempt = Attempt::new(1, 1, 1, bt_render::FrameSource::Expose, true, false, 0);
        attempt.outcome = Outcome::from_index(index);
        state.attempted(&attempt);
        assert_eq!(state.check(false, true, false, u64::MAX, false), None);
    }
    let mut state = State::default();
    state.observe(true, 0, Progress::default());
    assert_eq!(state.check(true, false, false, 100_000_000, false), None);
    assert_eq!(state.check(true, true, true, 100_000_000, false), None);
    assert_eq!(
        state.check(true, true, false, 100_000_000, false),
        Some(Line::Stale)
    );
    assert_eq!(state.check(true, true, false, 100_000_001, false), None);
    assert_eq!(state.age(100_000_001), 100_000_001);
}
