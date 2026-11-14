//! The macOS identity door (0.4.6 ticket U-16): the grammars over recorded
//! answers on every platform, and the real `codesign` over bundles built and
//! ad-hoc signed here, on macOS.
//!
//! The recorded answers are the shapes `codesign` and `spctl` gave on macOS
//! 26.6 on 2026-09-26, with the names and paths replaced by synthetic ones.

use super::*;

const TARGET: &str = "/tmp/staging/Folio.app";

fn verified() -> Result<Verified, Refusal> {
    Ok(Verified(()))
}

fn validity_refused() -> Result<Verified, Refusal> {
    Err(Refusal {
        stage: Stage::Validity,
        why: Why::Failed(Some(1)),
        first_line: Some("a sealed resource is missing or invalid".to_owned()),
    })
}

/// RED (U-16) — **a bundle Gatekeeper explicitly rejects is refused, however
/// valid its code and however well it matches the requirement.**
///
/// F-9: "a recorded Gatekeeper rejection is not acceptance". The rejection is
/// read from `spctl`'s recorded answer (the bundle path, `rejected`, exit 3)
/// through the real grammar, and handed with a verified bundle to the
/// decision: the answer is a refusal at the assessment stage that says it was
/// a rejection and carries the rule `spctl` named.
///
/// MUTATION: in `identity_decision`, answer `Assessment::Rejected { .. }` with
/// `Ok(Identity::Notarized)` (a rejection treated as an acceptance).
#[test]
fn an_explicit_rejection_is_refused() {
    for (stderr, reason) in [
        (format!("{TARGET}: rejected\n"), None),
        (
            format!("{TARGET}: rejected\nsource=no usable signature\n"),
            Some("no usable signature"),
        ),
        (
            format!(
                "{TARGET}: rejected\nsource=Unnotarized Developer ID\norigin=Developer ID Application: Example Ltd (ABCDE12345)\n"
            ),
            Some("Unnotarized Developer ID"),
        ),
        (
            format!("{TARGET}: rejected (the code is valid but does not seem to be an app)\n"),
            Some("the code is valid but does not seem to be an app"),
        ),
    ] {
        let assessment = parse_assessment(TARGET, Some(3), &stderr);
        assert_eq!(
            assessment,
            Some(Assessment::Rejected {
                reason: reason.map(str::to_owned)
            }),
            "{stderr:?}"
        );
        let refusal = identity_decision(verified(), Ok(assessment.unwrap())).unwrap_err();
        assert_eq!(refusal.stage, Stage::GatekeeperAssessment);
        assert_eq!(refusal.why, Why::Rejected);
        assert_eq!(refusal.first_line.as_deref(), reason);
    }
}

/// RED (U-16) — **with Gatekeeper's assessments turned off, a bundle passes on
/// its code validity and the requirement alone, and the pass says so; with
/// them off and the verification failed, it is refused.**
///
/// F-9: "assessments disabled → proceed on code validity plus the designated
/// requirement, recorded. The person turned Gatekeeper off; this is not ours to
/// override." Recorded means the value is `GatekeeperOff`, never `Notarized`.
/// Disabled is read from `spctl --status`'s recorded answer through the real
/// grammar.
///
/// MUTATION: in `identity_decision`, answer `Assessment::Disabled` with
/// `Ok(Identity::Notarized)` (the first half goes red), or move the
/// `verify?` below the match so a disabled Gatekeeper passes an unverified
/// bundle (the second half goes red).
#[test]
fn disabled_assessment_proceeds_on_code_and_requirement() {
    assert_eq!(parse_status("assessments disabled\n"), Some(false));
    assert_eq!(
        identity_decision(verified(), Ok(Assessment::Disabled)),
        Ok(Identity::GatekeeperOff)
    );
    let refusal = identity_decision(validity_refused(), Ok(Assessment::Disabled)).unwrap_err();
    assert_eq!(refusal.stage, Stage::Validity);
}

/// RED (U-16) — **a notarized Developer ID acceptance of a verified bundle
/// passes as `Notarized`, and an unverified bundle is refused however
/// Gatekeeper accepted it.**
///
/// §E: `spctl` answers "some notarized Developer ID", which is not "Folio", so
/// the identity check is required on this path too.
///
/// MUTATION: in `identity_decision`, drop `verify?` (take the verification as
/// given).
#[test]
fn a_notarized_developer_id_acceptance_passes_only_with_the_identity() {
    let stderr = format!(
        "{TARGET}: accepted\nsource=Notarized Developer ID\norigin=Developer ID Application: Example Ltd (ABCDE12345)\n"
    );
    let assessment = parse_assessment(TARGET, Some(0), &stderr).unwrap();
    assert_eq!(
        assessment,
        Assessment::Accepted {
            source: NOTARIZED_DEVELOPER_ID.to_owned()
        }
    );
    assert_eq!(
        identity_decision(verified(), Ok(assessment.clone())),
        Ok(Identity::Notarized)
    );
    assert_eq!(
        identity_decision(validity_refused(), Ok(assessment))
            .unwrap_err()
            .stage,
        Stage::Validity
    );
}

/// RED (U-16) — **Gatekeeper accepting a bundle under any rule but *Notarized
/// Developer ID* is refused, and the refusal names the rule.**
///
/// F-9 passes one acceptance. `Apple System` is what `spctl` says of the
/// system's own applications; `Mac App Store` and a developer's local rule
/// are acceptances too, and none of them is a notarized Folio.
///
/// MUTATION: in `identity_decision`, answer every `Assessment::Accepted` with
/// `Ok(Identity::Notarized)`.
#[test]
fn an_acceptance_from_another_source_is_refused() {
    for source in ["Apple System", "Mac App Store", "Unnotarized Developer ID"] {
        let stderr =
            format!("{TARGET}: accepted\nsource={source}\norigin=macOS Software Signing\n");
        let assessment = parse_assessment(TARGET, Some(0), &stderr).unwrap();
        let refusal = identity_decision(verified(), Ok(assessment)).unwrap_err();
        assert_eq!(refusal.stage, Stage::GatekeeperAssessment);
        assert_eq!(refusal.why, Why::NotNotarizedDeveloperId(source.to_owned()));
    }
}

/// RED (U-16) — **an answer outside `spctl`'s grammar is no answer, and the
/// decision refuses it.**
///
/// Each case is a way the recorded shape can go wrong: `spctl`'s own failure
/// line (exit 1, measured on a bundle with a missing sealed resource and on a
/// changed `Info.plist`), another path, an acceptance with no source, an exit
/// status that contradicts the verdict, an unknown or repeated key, and
/// nothing at all.
///
/// MUTATION: in `parse_assessment`, drop the `status == Some(0)` check on
/// `accepted` (the contradicting-status case goes red).
#[test]
fn malformed_assessment_output_is_refused() {
    let cases: [(Option<i32>, String); 9] = [
        (
            Some(1),
            format!("{TARGET}: a sealed resource is missing or invalid\n"),
        ),
        (
            Some(1),
            format!("{TARGET}: invalid Info.plist (plist or signature have been modified)\n"),
        ),
        (
            Some(0),
            "/tmp/elsewhere/Folio.app: accepted\nsource=Notarized Developer ID\n".to_owned(),
        ),
        (Some(0), format!("{TARGET}: accepted\n")),
        (
            Some(3),
            format!("{TARGET}: accepted\nsource=Notarized Developer ID\n"),
        ),
        (Some(0), format!("{TARGET}: rejected\n")),
        (
            Some(0),
            format!("{TARGET}: accepted\nsource=Notarized Developer ID\nverdict=yes\n"),
        ),
        (
            Some(0),
            format!("{TARGET}: accepted\nsource=Apple System\nsource=Notarized Developer ID\n"),
        ),
        (None, String::new()),
    ];
    for (status, stderr) in cases {
        assert_eq!(
            parse_assessment(TARGET, status, &stderr),
            None,
            "{stderr:?}"
        );
    }
    let malformed = Refusal::quoting(Stage::GatekeeperAssessment, Why::Malformed, "");
    assert_eq!(
        identity_decision(verified(), Err(malformed.clone())),
        Err(malformed)
    );
}

/// RED (U-16) — **`spctl --status` is one of two sentences; anything else is
/// no answer.**
///
/// MUTATION: in `parse_status`, answer `Some(false)` for anything that is not
/// `assessments enabled` (an unreadable status taken as "disabled" would pass
/// every verified bundle as `GatekeeperOff`).
#[test]
fn the_gatekeeper_status_is_one_of_two_sentences() {
    assert_eq!(parse_status("assessments enabled\n"), Some(true));
    assert_eq!(parse_status("assessments disabled\n"), Some(false));
    for other in [
        "",
        "assessments\n",
        "assessments enabled and disabled\n",
        "disabled\n",
    ] {
        assert_eq!(parse_status(other), None, "{other:?}");
    }
}

/// RED (U-16) — **the designated requirement is the one `designated =>` line,
/// explicit or implicit; none, two, or a line of another shape is no answer.**
///
/// The explicit shape is a notarized application's (synthetic identifier and
/// team); the implicit one, after `# `, is what an ad-hoc or linker-signed
/// binary carries.
///
/// MUTATION: in `parse_designated`, return the first `designated` line without
/// refusing a second one.
#[test]
fn the_designated_requirement_is_its_one_line() {
    let explicit = "identifier \"io.example.app\" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = \"ABCDE12345\"";
    assert_eq!(
        parse_designated(&format!("designated => {explicit}\n")).as_deref(),
        Some(explicit)
    );
    let implicit = "cdhash H\"32215c32d8971830458358ae7c1508d6013def23\"";
    assert_eq!(
        parse_designated(&format!("# designated => {implicit}\n")).as_deref(),
        Some(implicit)
    );
    assert_eq!(
        parse_designated(&format!("host => anchor apple\ndesignated => {explicit}\n")).as_deref(),
        Some(explicit)
    );
    for other in [
        String::new(),
        "host => anchor apple\n".to_owned(),
        format!("designated => {explicit}\ndesignated => {implicit}\n"),
        format!("Executable=/tmp/x\ndesignated => {explicit}\n"),
        "designated => \n".to_owned(),
    ] {
        assert_eq!(parse_designated(&other), None, "{other:?}");
    }
}

/// RED (U-16) — **a refusal carries at most the first line of what a tool
/// said, cut to its bound.**
///
/// The brief: "never quotes the tool's output beyond a bounded first line".
///
/// MUTATION: in `first_line`, drop `.take(QUOTE_BOUND)`.
#[test]
fn a_refusal_quotes_at_most_a_bounded_first_line() {
    let long = "x".repeat(QUOTE_BOUND * 3);
    let refusal = Refusal::quoting(
        Stage::Validity,
        Why::Failed(Some(1)),
        &format!("\n  {long}\nsecond line\n"),
    );
    assert_eq!(refusal.first_line.as_deref(), Some(&long[..QUOTE_BOUND]));
}

/// RED (U-16) — **the requirement a bundle is tested against demands the
/// Developer ID chain beside the running code's designated requirement.**
///
/// The text pin of what [`an_ad_hoc_bundle_is_refused_by_the_developer_id_requirement`]
/// runs for real on macOS; this half runs everywhere.
///
/// MUTATION: in `Requirement::text`, return the designated requirement alone.
#[test]
fn the_tested_requirement_is_the_designated_one_and_developer_id() {
    let requirement = Requirement::of_designated("identifier \"io.example.app\"".to_owned());
    assert_eq!(
        requirement.text(),
        format!("(identifier \"io.example.app\") and ({DEVELOPER_ID})")
    );
}

/// **Off macOS, every call is refused by name at its own stage.**
#[cfg(not(target_os = "macos"))]
#[test]
fn off_macos_every_call_is_refused_by_name() {
    let refusal = running_requirement().unwrap_err();
    assert_eq!(
        (refusal.stage, refusal.why),
        (Stage::RunningRequirement, Why::NotMacOs)
    );
    let requirement = Requirement::of_designated("identifier \"io.example.app\"".to_owned());
    let refusal = verify_bundle(Path::new(TARGET), &requirement).unwrap_err();
    assert_eq!(
        (refusal.stage, refusal.why),
        (Stage::Validity, Why::NotMacOs)
    );
    let refusal = assess(Path::new(TARGET)).unwrap_err();
    assert_eq!(
        (refusal.stage, refusal.why),
        (Stage::GatekeeperStatus, Why::NotMacOs)
    );
}

#[cfg(target_os = "macos")]
mod on_macos {
    use super::super::*;
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    /// **A clock whose deadline never comes** (T-CODESIGN-CHILD-BOUND-ON-A-CLOCK): its reading is
    /// pinned where it was made, so the deadline a call sets on it is never reached, and its rest
    /// is the product's own ([`WallLockClock`]'s), so the child runs. A real `codesign` or
    /// `spctl` handed it is answered by its own exit, awaited as state, never raced by its bound;
    /// one that never exits is the suite watchdog's to name. Each call is handed a clock of its
    /// own, so no reading is shared between tests.
    struct ExitAwaited {
        pinned: Instant,
    }

    impl ExitAwaited {
        fn new() -> Self {
            Self {
                pinned: Instant::now(),
            }
        }
    }

    impl LockClock for ExitAwaited {
        fn now(&self) -> Instant {
            self.pinned
        }

        fn pause(&self, pause: Duration) {
            WallLockClock.pause(pause);
        }
    }

    /// What a [`CountedRests`] clock does with one rest between two looks at the child.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum RestTurn {
        /// Rest a real moment without moving the reading: the child's own movement is awaited as
        /// state, within the suite's patience ([`crate::door_skipped::PATIENCE`]).
        Await,
        /// Move the reading by this much at once, without waiting.
        Spend(Duration),
    }

    /// How many awaited rests make the suite's patience.
    const PATIENT_LOOKS: u32 =
        (crate::door_skipped::PATIENCE.as_millis() / arm::POLL.as_millis()) as u32;

    /// **A test's [`LockClock`]**: its reading moves only by what `turn` spends, so a child's
    /// bound ends at a count of the test's and never on real time. An awaited rest is a real
    /// rest with the reading unmoved; past the suite's patience of them the clock gives up — it
    /// spends a whole hour, so the call ends — and the test fails by name (`gave_up`), never
    /// hangs.
    struct CountedRests {
        began: Instant,
        moved: Cell<Duration>,
        spent: Cell<u32>,
        awaited: Cell<u32>,
        gave_up: Cell<bool>,
        turn: RefCell<Box<dyn FnMut() -> RestTurn>>,
    }

    impl CountedRests {
        fn new(turn: impl FnMut() -> RestTurn + 'static) -> Self {
            Self {
                began: Instant::now(),
                moved: Cell::new(Duration::ZERO),
                spent: Cell::new(0),
                awaited: Cell::new(0),
                gave_up: Cell::new(false),
                turn: RefCell::new(Box::new(turn)),
            }
        }
    }

    impl LockClock for CountedRests {
        fn now(&self) -> Instant {
            self.began + self.moved.get()
        }

        fn pause(&self, pause: Duration) {
            let turn = (self.turn.borrow_mut())();
            match turn {
                RestTurn::Spend(by) => {
                    self.spent.set(self.spent.get() + 1);
                    self.moved.set(self.moved.get() + by);
                }
                RestTurn::Await if self.awaited.get() < PATIENT_LOOKS => {
                    self.awaited.set(self.awaited.get() + 1);
                    WallLockClock.pause(pause);
                }
                RestTurn::Await => {
                    self.gave_up.set(true);
                    self.moved.set(self.moved.get() + Duration::from_secs(3600));
                }
            }
        }
    }

    /// The scripted children's shell, part of macOS: the bound's tests start it, never a real
    /// `codesign`.
    const SH: &str = "/bin/sh";

    /// **A child that answers only after its bound**: it says `standing`, marks `$1` so the test
    /// knows it has, then waits for `$2` — the test's late answer — and only then says `late`.
    const LATE_CHILD: &str =
        r#"printf standing; : > "$1"; while [ ! -e "$2" ]; do sleep 0.01; done; printf late"#;

    /// A fresh folder for one test, under the system's temporary folder.
    fn scratch(name: &str) -> PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-platform-macos-identity-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    /// **A synthetic application bundle, ad-hoc signed here** — this test
    /// binary copied in as its executable, a minimal `Info.plist`, and
    /// `codesign -s -` over the whole. Nothing of Folio's is read or signed.
    fn ad_hoc_bundle(root: &Path) -> PathBuf {
        let bundle = root.join("Probe.app");
        let executables = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&executables).unwrap();
        std::fs::copy(std::env::current_exe().unwrap(), executables.join("probe")).unwrap();
        std::fs::write(
            bundle.join("Contents/Info.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\n\
             <key>CFBundleExecutable</key><string>probe</string>\n\
             <key>CFBundleIdentifier</key><string>io.example.identity-probe</string>\n\
             <key>CFBundlePackageType</key><string>APPL</string>\n\
             </dict></plist>\n",
        )
        .unwrap();
        let signed = crate::quiet_command(CODESIGN)
            .args(["-s", "-", "--force"])
            .arg(&bundle)
            .output()
            .unwrap();
        assert!(signed.status.success(), "{signed:?}");
        bundle
    }

    /// RED (U-16) — **an ad-hoc signed bundle is refused by the Developer ID
    /// requirement: its code is valid and it satisfies its own designated
    /// requirement, and it still fails at the requirement stage.**
    ///
    /// The real negative test the brief asks for. The bundle is built and
    /// signed here; its designated requirement is read by the real reader,
    /// and the real verification runs twice: against that requirement alone
    /// it passes (so the signature, the seal and the reader are sound), and
    /// against the [`Requirement`] the updater uses — the same designated
    /// requirement and [`DEVELOPER_ID`] — it is refused as not satisfied.
    ///
    /// MUTATION: in `Requirement::text`, return the designated requirement
    /// alone.
    #[test]
    fn an_ad_hoc_bundle_is_refused_by_the_developer_id_requirement() {
        let root = scratch("ad-hoc");
        let bundle = ad_hoc_bundle(&root);
        let designated =
            designated_requirement_of(&bundle.to_string_lossy(), &ExitAwaited::new()).unwrap();
        assert!(designated.starts_with("cdhash H\""), "{designated}");

        assert_eq!(
            verify_against(&bundle, &designated, &ExitAwaited::new()),
            Ok(Verified(()))
        );
        let refusal = verify_bundle_on(
            &bundle,
            &Requirement::of_designated(designated),
            &ExitAwaited::new(),
        )
        .unwrap_err();
        assert_eq!(
            (refusal.stage, refusal.why.clone()),
            (Stage::Requirement, Why::NotSatisfied),
            "{refusal}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// RED (U-16) — **a bundle changed after it was signed fails at the
    /// validity stage, before any requirement is asked about.**
    ///
    /// MUTATION: in `verify_against`, skip the first `codesign` run (the
    /// requirement run then refuses with exit 1 at `Stage::Requirement`).
    #[test]
    fn a_bundle_changed_after_signing_fails_code_validity() {
        let root = scratch("changed");
        let bundle = ad_hoc_bundle(&root);
        let designated =
            designated_requirement_of(&bundle.to_string_lossy(), &ExitAwaited::new()).unwrap();
        let plist = bundle.join("Contents/Info.plist");
        let mut text = std::fs::read_to_string(&plist).unwrap();
        text.push('\n');
        std::fs::write(&plist, text).unwrap();

        let refusal = verify_against(&bundle, &designated, &ExitAwaited::new()).unwrap_err();
        assert_eq!(
            (refusal.stage, refusal.why.clone()),
            (Stage::Validity, Why::Failed(Some(1))),
            "{refusal}"
        );
        assert!(refusal.first_line.is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    /// RED (U-16) — **the running requirement is read from the running test
    /// binary: it is that binary's own designated requirement, and the
    /// Developer ID half makes the binary refuse itself.**
    ///
    /// `cargo test` binaries on Apple silicon are linker-signed (ad hoc), so
    /// this is also what an ad-hoc running build of Folio reads: a requirement
    /// nothing ad-hoc satisfies, including the build itself.
    ///
    /// MUTATION: in `running_requirement`, read the requirement of any other
    /// code (e.g. `/usr/bin/true`): the executable no longer satisfies the
    /// designated half and `verify_against` refuses it.
    #[test]
    fn the_running_requirement_is_read_from_the_running_code() {
        let running = running_requirement_on(&ExitAwaited::new()).unwrap();
        let executable = std::env::current_exe().unwrap();
        assert_eq!(
            verify_against(&executable, running.designated(), &ExitAwaited::new()),
            Ok(Verified(()))
        );
        let refusal = verify_bundle_on(&executable, &running, &ExitAwaited::new()).unwrap_err();
        assert_eq!(
            (refusal.stage, refusal.why.clone()),
            (Stage::Requirement, Why::NotSatisfied),
            "{refusal}"
        );
    }

    /// RED (U-16) — **Gatekeeper's answer about an ad-hoc bundle is read from
    /// the real `spctl`: rejected when assessments are on, disabled when they
    /// are off, and never an acceptance.**
    ///
    /// Assessed: a synthetic bundle built here, never an installed Folio.
    ///
    /// MUTATION: in `parse_assessment`, answer `Accepted` for `rejected`.
    #[test]
    fn an_ad_hoc_bundle_is_never_accepted_by_spctl() {
        let root = scratch("assess");
        let bundle = ad_hoc_bundle(&root);
        match assess_on(&bundle, &ExitAwaited::new()).unwrap() {
            Assessment::Rejected { .. } | Assessment::Disabled => {}
            accepted @ Assessment::Accepted { .. } => panic!("{accepted:?}"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// RED (T-CODESIGN-CHILD-BOUND-ON-A-CLOCK) — **a child that would answer only after its bound
    /// is ended at the bound's count, and the refusal quotes what it had said by then.**
    ///
    /// A scripted child, not a real `codesign`: it says `standing`, marks that it has, and waits
    /// for the test's late answer. The clock awaits the mark (reading unmoved), then spends
    /// `arm::POLL` per rest; only a rest past [`DISPLAY_BOUND`]'s count lets the late answer in.
    /// The call is refused [`Why::TimedOut`] after exactly `DISPLAY_BOUND / POLL` spent rests, the
    /// late answer never let in, and the quote is `standing` (0.4.8 E6).
    ///
    /// MUTATION: in `arm::run`, read the deadline's check on `Instant::now()` (the real clock),
    /// delete the check, or set the deadline at twice the bound — each lets the late answer in,
    /// and the call answers `Ok`.
    #[test]
    fn a_child_that_answers_past_its_bound_is_ended_and_quoted_at_its_count() {
        let root = scratch("late");
        let (said, late) = (root.join("standing"), root.join("late"));
        let in_bound = u32::try_from(DISPLAY_BOUND.as_millis() / arm::POLL.as_millis()).unwrap();
        let (seen, answer) = (said.clone(), late.clone());
        let mut spent = 0;
        let clock = CountedRests::new(move || {
            if !seen.exists() {
                return RestTurn::Await;
            }
            if spent < in_bound {
                spent += 1;
                return RestTurn::Spend(arm::POLL);
            }
            std::fs::write(&answer, b"").unwrap();
            RestTurn::Await
        });
        let (said_text, late_text) = (said.to_string_lossy(), late.to_string_lossy());
        let arguments: [&str; 5] = ["-c", LATE_CHILD, "late-child", &said_text, &late_text];

        let refusal = arm::run(
            Stage::GatekeeperStatus,
            SH,
            &arguments,
            DISPLAY_BOUND,
            &clock,
        )
        .map(|answer| answer.stdout)
        .unwrap_err();
        assert_eq!(
            (refusal.stage, refusal.why.clone()),
            (Stage::GatekeeperStatus, Why::TimedOut),
            "{refusal}"
        );
        assert_eq!(refusal.first_line.as_deref(), Some("standing"), "{refusal}");
        assert!(!clock.gave_up.get(), "the child never said it stood");
        assert_eq!(
            (clock.spent.get(), clock.moved.get()),
            (in_bound, DISPLAY_BOUND)
        );
        assert!(!late.exists(), "the late answer was let in");
        let _ = std::fs::remove_dir_all(root);
    }

    /// RED (T-CODESIGN-CHILD-BOUND-ON-A-CLOCK) — **a child that answers inside its bound is read:
    /// its exit status and both of its streams, as text.**
    ///
    /// A scripted child on [`ExitAwaited`], whose deadline never comes: the answer is the child's
    /// own exit, awaited as state.
    ///
    /// MUTATION: hand this test a clock already spent past its bound at the first rest (the
    /// zero-budget plant): the child is ended before it answers.
    #[test]
    fn an_answer_inside_the_bound_is_read_with_its_status_and_both_streams() {
        let answer = arm::run(
            Stage::GatekeeperStatus,
            SH,
            &["-c", "printf said; printf err >&2; exit 3"],
            DISPLAY_BOUND,
            &ExitAwaited::new(),
        )
        .map(|answer| (answer.status, answer.stdout, answer.stderr));
        assert_eq!(answer, Ok((Some(3), "said".to_owned(), "err".to_owned())));
    }
}
