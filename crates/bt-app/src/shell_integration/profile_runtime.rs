//! Discovery, startup scheduling and the shared non-interactive removal door.
use super::{profile_marks::*, *};
use crate::i18n::Text;
use std::{io, sync::Mutex};

static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static REMOVAL: Mutex<Option<Report>> = Mutex::new(None);

/// A sandbox replaces the entire candidate set, including recorded real paths.
/// An empty override refuses discovery rather than falling through to the user.
fn sandbox_profile() -> io::Result<Option<PathBuf>> {
    std::env::var_os("BT_POWERSHELL_PROFILE")
        .map(|raw| {
            let path = PathBuf::from(raw);
            if !path.is_absolute() {
                Err(io::Error::other(Text::ShellMarksPath.text()))
            } else {
                Ok(path)
            }
        })
        .transpose()
}

fn script_at(data: &Path) -> PathBuf {
    data.join(SCRIPT_DIRECTORY).join(SCRIPT_FILE_PS1)
}

pub fn startup_needed(record_exists: bool, script_exists: bool) -> bool {
    record_exists || script_exists
}

fn schedule_migration(record_exists: bool, script_exists: bool, start: impl FnOnce()) {
    if startup_needed(record_exists, script_exists) {
        start();
    }
}

/// Only two metadata questions on the caller. No profile read, process query or
/// migration on the window thread; pristine installs don't even start a worker.
pub fn begin_startup_migration() {
    if STARTED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    let data = persist::storage_dir();
    schedule_migration(
        data.join(RECORD_FILE).exists(),
        script_at(&data).exists(),
        || {
            let _ = bt_platform::spawn_at_priority(
                "powershell-profile-migration",
                bt_platform::ThreadPriority::BelowNormal,
                move || {
                    let report = operate(&data, Asker::InApp, Action::Migrate);
                    for refusal in report.refusals() {
                        eprintln!(
                            "BT_SHELL_PROFILE {}: {}",
                            refusal.path.display(),
                            refusal.reason
                        );
                    }
                    if let Some(wake) = WAKE.get() {
                        wake();
                    }
                },
            );
        },
    );
}

fn candidates(marks: &Marks) -> (Vec<PathBuf>, Report) {
    let mut report = Report::default();
    match sandbox_profile() {
        Ok(Some(path)) => return (vec![path], report),
        Err(e) => {
            report.files.push(FileReport {
                path: PathBuf::from("BT_POWERSHELL_PROFILE"),
                fate: Fate::Refused(e.to_string()),
            });
            return (Vec::new(), report);
        }
        Ok(None) => {}
    }
    // Recorded paths are checked before they are used, here and not only under a
    // sandbox: see `recorded_profile_is_usable`. A refused one is named and left.
    let mut paths: Vec<PathBuf> = Vec::new();
    for path in marks
        .powershell_profiles
        .iter()
        .chain(marks.profile_refusals.iter().map(|refusal| &refusal.path))
    {
        if !recorded_profile_is_usable(path) {
            report.files.push(FileReport {
                path: path.clone(),
                fate: Fate::Refused(Text::ShellMarksProfileKind.text().to_owned()),
            });
        } else if !paths.contains(path) {
            paths.push(path.clone());
        }
    }
    for program in installed_powershells() {
        match cached_profile_answer(&program) {
            Some(path) => {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            None => report.files.push(FileReport {
                path: program,
                fate: Fate::Refused(Text::ShellProfileProbeFailed.text().to_owned()),
            }),
        }
    }
    (paths, report)
}

/// Public for T-C1: one account operation, no GUI/settings/handoff, partial
/// results retained. Call on its cleanup worker or early CLI path.
///
/// `asker` says which of the two this run is: the Settings row's own worker
/// inside a running Folio, or somebody at a command line in a process of their
/// own. See [`Asker`].
pub fn remove_shell_integration(asker: Asker) -> Report {
    operate(&persist::storage_dir(), asker, Action::Remove)
}

/// The cleanup door supplies its resolved data root without triggering storage migration.
/// An explicit profile set replaces historical and probed paths, just like the sandbox env door.
pub fn remove_shell_integration_at(data: &Path, profiles: Option<&[PathBuf]>) -> Report {
    // A door, by the name on it: this is `--uninstall-cleanup`'s road, and it has
    // already refused if a Folio is running. Only a run that will ask the machine
    // where its profiles are pays for asking.
    if profiles.is_none() {
        warm_profile_answers();
    }
    operate_with(
        data,
        Asker::Door,
        Action::Remove,
        Ok(MANAGED_LINE),
        |marks| {
            profiles.map_or_else(
                || candidates(marks),
                |paths| (paths.to_vec(), Report::default()),
            )
        },
    )
}

/// **Ask the machine its slow question before the record is locked.**
///
/// [`candidates`] asks each installed PowerShell where its own `$PROFILE` is,
/// and that answer costs a child process with a five-second deadline apiece.
/// Asked where it used to be asked — inside [`operate_with`], under the lock —
/// one of Folio's own writers could hold the record for ten seconds while two
/// shells started, which is the difference between a turn worth waiting for and
/// a wait nobody can be asked to make on a window thread. The answer is a
/// property of the installation and not of the record, and
/// `cached_profile_answer` keeps it for the life of the process, so asking it
/// here leaves `candidates` reading a cache it would have filled anyway.
fn warm_profile_answers() {
    // The sandbox door replaces the whole candidate set, so no shell is asked.
    if std::env::var_os("BT_POWERSHELL_PROFILE").is_some() {
        return;
    }
    for program in installed_powershells() {
        let _ = cached_profile_answer(&program);
    }
}

fn operate(data: &Path, asker: Asker, action: Action) -> Report {
    let managed = if action == Action::Remove {
        Ok(MANAGED_LINE)
    } else {
        account_managed_line(data)
    };
    warm_profile_answers();
    operate_with(data, asker, action, managed, |marks| {
        // This closure is reached only while enabled, under the same account lock
        // as removal. Off cannot race a late script repair.
        if action == Action::Migrate && std::env::var_os("BT_POWERSHELL_PROFILE").is_none() {
            let _ = powershell_script_repaired();
        }
        candidates(marks)
    })
}

fn operate_with(
    data: &Path,
    asker: Asker,
    action: Action,
    managed: io::Result<&'static str>,
    discover: impl FnOnce(&Marks) -> (Vec<PathBuf>, Report),
) -> Report {
    let record_path = data.join(RECORD_FILE);
    let refused_record = |error: io::Error| Report {
        files: vec![FileReport {
            path: record_path.clone(),
            fate: Fate::Refused(error.to_string()),
        }],
    };
    // An uninstaller must not create anything, and this is the door it runs through.
    // A data root that does not exist holds no marks and has no decision to keep, so
    // the run reads the default record and writes nothing — the root is still absent
    // afterwards. Discovery and removal still run: a `$PROFILE` line outlives the data
    // folder, and an account that never had one simply has nothing to report.
    let _lock = match lock_existing(data, asker) {
        Ok(lock) => lock,
        Err(e) => return refused_record(e),
    };
    let records = _lock.is_some();
    let mut marks = match Marks::read(data) {
        Ok(marks) => marks,
        Err(e) => return refused_record(e),
    };
    if action == Action::Migrate && marks.is_off() {
        return Report::default();
    }
    let managed = match managed {
        Ok(managed) => managed,
        Err(e) => return refused_record(e),
    };
    if action == Action::Remove && records {
        marks.turn_off(std::time::SystemTime::now());
        if let Err(e) = marks.write(data) {
            return refused_record(e);
        }
    }
    let (paths, mut report) = discover(&marks);
    let script = script_at(data);
    let mut scripts = marks.powershell_scripts.clone();
    if !scripts.contains(&script) {
        scripts.push(script.clone());
    }
    let forms = Forms::new(&scripts).targeting(managed);

    // Discovery is not ownership. Remember only an exact mark encountered by
    // the applier's existing scan, before it can replace the profile. Retain old
    // record locations as historical retry candidates, never as edit authority.
    report.files.extend(
        apply_recorded(&paths, &forms, action, |path| {
            marks.remember(path, &script);
            if records { marks.write(data) } else { Ok(()) }
        })
        .files,
    );
    // Probe refusals name executables, not profiles. They are reported on this
    // run, and retried through discovery, never treated as profile candidates.
    marks.profile_refusals = report
        .refusals()
        .into_iter()
        .filter(|r| paths.contains(&r.path))
        .collect();
    // Merely discovering a hand-written installation must not create an
    // enabled record. Existing records and explicit Off decisions still persist.
    if records
        && (record_path.exists() || !marks.profile_refusals.is_empty())
        && let Err(e) = marks.write(data)
    {
        report.files.extend(refused_record(e).files);
    }
    report
}

/// Install's record transaction. Unit tests inject both profile and data root.
pub fn install_recorded(
    profile: &Path,
    data: &Path,
    script: &Path,
    line: &'static str,
    at: std::time::SystemTime,
) -> io::Result<ProfileWrite> {
    let profile = std::path::absolute(profile)?;
    let script = std::path::absolute(script)?;
    let _lock = lock(data, Asker::InApp)?;
    let mut marks = Marks::read(data)?;
    marks.powershell_state = PowerShellState::Enabled {};
    marks.remember(&profile, &script);
    marks.write(data)?;
    let result = add_profile_with_forms(
        &profile,
        line,
        &Forms::new(&marks.powershell_scripts).targeting(line),
        at,
    );
    marks.profile_refusals.retain(|r| r.path != profile);
    if let Err(e) = &result {
        marks.profile_refusals.push(Refusal {
            path: profile,
            reason: e.to_string(),
        });
    }
    marks.write(data)?;
    result
}

fn enable_record(data: &Path) -> io::Result<()> {
    let _lock = lock(data, Asker::InApp)?;
    let mut marks = Marks::read(data)?;
    marks.powershell_state = PowerShellState::Enabled {};
    marks.write(data)
}

pub fn begin_enable() {
    let _ = bt_platform::spawn_at_priority(
        "powershell-profile-enable",
        bt_platform::ThreadPriority::BelowNormal,
        || {
            let data = persist::storage_dir();
            if let Err(error) = enable_record(&data) {
                let report = Report {
                    files: vec![FileReport {
                        path: data.join(RECORD_FILE),
                        fate: Fate::Refused(error.to_string()),
                    }],
                };
                if let Ok(mut outcome) = REMOVAL.lock() {
                    *outcome = Some(report);
                }
                if let Some(wake) = WAKE.get() {
                    wake();
                }
            }
        },
    );
}

pub fn begin_removal() {
    let _ = bt_platform::spawn_at_priority(
        "powershell-profile-removal",
        bt_platform::ThreadPriority::BelowNormal,
        || {
            let report = remove_shell_integration(Asker::InApp);
            if let Ok(mut outcome) = REMOVAL.lock() {
                *outcome = Some(report);
            }
            if let Some(wake) = WAKE.get() {
                wake();
            }
        },
    );
}

pub fn take_removal() -> Option<Report> {
    REMOVAL.lock().ok()?.take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn one(fate: Fate) -> Report {
        Report {
            files: vec![FileReport {
                path: PathBuf::from(r"C:\Users\alice\profile.ps1"),
                fate,
            }],
        }
    }

    /// PIN — **a removal that found nothing to remove says nothing in the
    /// window**, and a refusal or a real removal still speaks.
    ///
    /// The rule is [`Report::window_text`]'s and not the toast call site's,
    /// because the call site is what broke it: an empty successful report was
    /// given the words `No Folio profile lines found.`, and the first-run
    /// card's PowerShell row left off presses the same `Off` the Settings page
    /// sends, which runs a removal — so a brand-new machine's first sight of
    /// Folio was a message about a `$PROFILE` line it never had.
    ///
    /// MUTATIONS:
    /// ① give the empty report words again and `Done` greets a new reader with
    ///    a toast about nothing;
    /// ② drop the refusal flag and a removal that was refused arrives in the
    ///    corner as an `Ok`, with the refusal's own sentence missing besides.
    #[test]
    fn shell_integration_removal_with_nothing_to_remove_tells_the_window_nothing() {
        assert_eq!(Report::default().window_text(), None);
        let unchanged = one(Fate::Unchanged);
        assert_eq!(unchanged.exit_code(), 0);
        assert_eq!(
            unchanged.window_text(),
            None,
            "a profile that was looked at and left alone is being announced"
        );
        let (refused, text) = one(Fate::Removed)
            .window_text()
            .expect("a line that was taken out of somebody's $PROFILE is news");
        assert!(!refused);
        assert!(text.contains(r"C:\Users\alice\profile.ps1"));
        assert!(text.contains(Text::ShellProfileRemoved.text()));
        let (refused, text) = one(Fate::Refused("the file is in use".to_owned()))
            .window_text()
            .expect("a removal that could not be done is always news");
        assert!(refused);
        assert!(text.contains("the file is in use"));
    }

    #[test]
    fn shell_integration_offer_predicate_never_grants_write_ownership() {
        let source = include_str!("../shell_integration.rs");
        let offer = source
            .split_once("pub fn offer_for(profile: &Path) -> Offer {")
            .unwrap()
            .1
            .split_once("\n}\n")
            .unwrap()
            .0;
        assert!(offer.contains("profile_suppresses_integration_offer(&decoded.text)"));
        let writer = source
            .split_once("fn add_profile_with_forms(")
            .unwrap()
            .1
            .split_once("\n}\n")
            .unwrap()
            .0;
        assert!(writer.contains("forms.owns("));
        assert!(!writer.contains("profile_suppresses_integration_offer"));
        let marks = include_str!("profile_marks.rs");
        assert!(!marks.contains("profile_suppresses_integration_offer"));
    }

    #[test]
    fn shell_integration_old_enabled_record_is_read_without_claiming_user_code() {
        let root = super::super::tests::temp_dir("old-enabled-handwritten");
        let profile = root.join("profile.ps1");
        let original = b". 'D:\\x\\folio.ps1'\r\n";
        fs::write(&profile, original).unwrap();
        let mut old = Marks::default();
        old.remember(&profile, &script_at(&root));
        old.write(&root).unwrap();
        assert!(!Marks::read(&root).unwrap().is_off());
        let report = operate_with(
            &root,
            Asker::InApp,
            Action::Remove,
            Ok(MANAGED_LINE),
            |marks| (marks.powershell_profiles.clone(), Report::default()),
        );
        assert_eq!(report.exit_code(), 0);
        assert_eq!(fs::read(&profile).unwrap(), original);
        assert!(report.text(false).is_empty());
        assert!(Marks::read(&root).unwrap().is_off());
    }

    #[test]
    fn shell_integration_offer_respects_handwritten_installation() {
        let root = super::super::tests::temp_dir("offer-loose");
        let profile = root.join("profile.ps1");
        for line in [r". 'D:\x\folio.ps1'", r". 'D:\x\FOLIO.PS1'", MANAGED_LINE] {
            fs::write(&profile, line).unwrap();
            assert_eq!(offer_for(&profile), Offer::Silent, "{line}");
        }
        fs::write(&profile, "  # . 'D:\\x\\folio.ps1'\r\n").unwrap();
        assert_eq!(offer_for(&profile), Offer::Owed(profile));
    }

    #[test]
    fn shell_integration_handwritten_profile_is_not_a_recorded_mark() {
        let root = super::super::tests::temp_dir("handwritten-record");
        let profile = root.join("profile.ps1");
        let original = b". 'D:\\x\\folio.ps1'\r\n";
        fs::write(&profile, original).unwrap();
        let report = operate_with(
            &root,
            Asker::InApp,
            Action::Migrate,
            Ok(MANAGED_LINE),
            |_| (vec![profile.clone()], Report::default()),
        );
        assert_eq!(report.exit_code(), 0);
        assert_eq!(report.files[0].fate, Fate::Unchanged);
        assert_eq!(fs::read(&profile).unwrap(), original);
        assert!(Marks::read(&root).unwrap().powershell_profiles.is_empty());
        assert!(!root.join(RECORD_FILE).exists());
    }

    #[test]
    fn shell_integration_handwritten_off_reports_nothing_and_preserves_bytes() {
        let root = super::super::tests::temp_dir("handwritten-off");
        let profile = root.join("profile.ps1");
        let original = b". 'D:\\x\\folio.ps1'\r\n";
        fs::write(&profile, original).unwrap();
        let report = operate_with(
            &root,
            Asker::InApp,
            Action::Remove,
            Ok(MANAGED_LINE),
            |_| (vec![profile.clone()], Report::default()),
        );
        assert_eq!(report.exit_code(), 0);
        assert_eq!(report.files[0].fate, Fate::Unchanged);
        assert_eq!(fs::read(&profile).unwrap(), original);
        // The CLI maps an empty successful report to ShellProfileNothing; the
        // window is told nothing at all. See `window_text`.
        assert!(report.text(false).is_empty());
        assert_eq!(report.window_text(), None);
        let marks = Marks::read(&root).unwrap();
        assert!(marks.is_off());
        assert!(marks.powershell_profiles.is_empty());
    }

    #[test]
    fn shell_integration_followup_legacy_root_keeps_working_script() {
        let root = super::super::tests::temp_dir("followup-legacy");
        let data = root.join("BetterTerminal");
        let script = script_at(&data);
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, "# working integration").unwrap();
        let profile = root.join("profile.ps1");
        fs::write(
            &profile,
            r#". "$env:APPDATA\BetterTerminal\shell-integration\folio.ps1""#,
        )
        .unwrap();
        let report = operate_with(
            &data,
            Asker::InApp,
            Action::Migrate,
            managed_line_for(&data, &root),
            |_| (vec![profile.clone()], Report::default()),
        );
        assert_eq!(report.exit_code(), 0);
        let after = fs::read_to_string(&profile).unwrap();
        assert!(after.starts_with("if (Test-Path -LiteralPath "));
        assert!(
            after.contains(r#"{ . "$env:APPDATA\BetterTerminal\shell-integration\folio.ps1" }"#),
            "{after}"
        );
        assert!(script.exists());
    }

    #[test]
    fn shell_integration_followup_off_is_persisted_and_skips_discovery() {
        let root = super::super::tests::temp_dir("followup-off");
        let profile = root.join("profile.ps1");
        fs::write(&profile, LEGACY_LINE).unwrap();
        let report = operate_with(
            &root,
            Asker::InApp,
            Action::Remove,
            Ok(MANAGED_LINE),
            |_| (vec![profile.clone()], Report::default()),
        );
        assert_eq!(report.exit_code(), 0);
        let before = fs::read(root.join(RECORD_FILE)).unwrap();
        let report = operate_with(
            &root,
            Asker::InApp,
            Action::Migrate,
            Ok(MANAGED_LINE),
            |_| panic!("off must not probe"),
        );
        assert_eq!(report.exit_code(), 0);
        assert_eq!(fs::read(root.join(RECORD_FILE)).unwrap(), before);
        let record: serde_json::Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(record["version"], 2);
        assert_eq!(record["powershell_state"]["state"], "off");
        assert_eq!(record["powershell_state"]["by"], "user");
        assert!(
            record["powershell_state"]["at"]
                .as_str()
                .unwrap()
                .ends_with('Z')
        );
    }

    #[test]
    fn shell_integration_followup_off_skips_script_repair_until_user_enables() {
        let root = super::super::tests::temp_dir("off-repair");
        let script = script_at(&root);
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, "user's existing script").unwrap();
        let report = operate_with(
            &root,
            Asker::InApp,
            Action::Remove,
            Ok(MANAGED_LINE),
            |_| (vec![], Report::default()),
        );
        assert_eq!(report.exit_code(), 0);
        for _ in 0..3 {
            let mut probes = 0;
            let mut writes = 0;
            let report = operate_with(
                &root,
                Asker::InApp,
                Action::Migrate,
                Ok(MANAGED_LINE),
                |_| {
                    probes += 2;
                    writes += 1;
                    fs::write(&script, "repaired").unwrap();
                    (vec![], Report::default())
                },
            );
            assert_eq!(report.exit_code(), 0);
            assert_eq!((probes, writes), (0, 0));
            assert_eq!(
                fs::read_to_string(&script).unwrap(),
                "user's existing script"
            );
        }
        enable_record(&root).unwrap();
        assert!(!Marks::read(&root).unwrap().is_off());
        let mut reached = false;
        operate_with(
            &root,
            Asker::InApp,
            Action::Migrate,
            Ok(MANAGED_LINE),
            |_| {
                reached = true;
                (vec![], Report::default())
            },
        );
        assert!(reached);
    }

    #[test]
    fn shell_integration_followup_schema_one_upgrades_without_losing_other_owners() {
        let root = super::super::tests::temp_dir("schema-one");
        let mut old = serde_json::to_value(Marks::default()).unwrap();
        old["version"] = serde_json::json!(1);
        old.as_object_mut().unwrap().remove("powershell_state");
        old["agent_config_roots"]["codex"] = serde_json::json!([root.join("codex")]);
        fs::write(root.join(RECORD_FILE), serde_json::to_vec(&old).unwrap()).unwrap();
        let marks = Marks::read(&root).unwrap();
        assert_eq!(marks.version, 2);
        assert!(!marks.is_off());
        marks.write(&root).unwrap();
        assert_eq!(
            Marks::read(&root).unwrap().agent_config_roots.codex,
            vec![root.join("codex")]
        );
        for state in [
            serde_json::json!({"state":"off","by":"someone","at":"2026-09-20T00:00:00Z"}),
            serde_json::json!({"state":"off","by":"user","at":"bad date"}),
            serde_json::json!({"state":"off","by":"user"}),
            serde_json::json!({"state":"enabled","extra":true}),
        ] {
            let mut invalid = serde_json::to_value(&marks).unwrap();
            invalid["powershell_state"] = state;
            let bytes = serde_json::to_vec(&invalid).unwrap();
            fs::write(root.join(RECORD_FILE), &bytes).unwrap();
            assert!(Marks::read(&root).is_err());
            assert_eq!(fs::read(root.join(RECORD_FILE)).unwrap(), bytes);
        }
    }

    #[test]
    fn shell_integration_never_enabled_does_not_schedule_or_probe() {
        let mut workers = 0;
        let mut probes = 0;
        schedule_migration(false, false, || {
            workers += 1;
            probes += 2;
        });
        assert_eq!((workers, probes), (0, 0));
        assert!(startup_needed(true, false));
        assert!(startup_needed(false, true));
    }

    #[test]
    fn shell_integration_record_captures_two_profiles_and_preserves_other_owners() {
        let root = super::super::tests::temp_dir("marks-record");
        let data = root.join("data");
        let script = script_at(&data);
        for name in ["5.1.ps1", "7.ps1"] {
            install_recorded(
                &root.join(name),
                &data,
                &script,
                MANAGED_LINE,
                std::time::UNIX_EPOCH,
            )
            .unwrap();
        }
        let mut marks = Marks::read(&data).unwrap();
        assert_eq!(marks.powershell_profiles.len(), 2);
        marks.agent_config_roots.codex.push(root.join("codex"));
        marks.psreadline_module_roots.push(root.join("module"));
        marks.write(&data).unwrap();
        install_recorded(
            &root.join("7.ps1"),
            &data,
            &script,
            MANAGED_LINE,
            std::time::UNIX_EPOCH,
        )
        .unwrap();
        let reread = Marks::read(&data).unwrap();
        assert_eq!(reread.powershell_profiles.len(), 2);
        assert_eq!(
            reread.agent_config_roots.codex,
            marks.agent_config_roots.codex
        );
        assert_eq!(
            reread.psreadline_module_roots,
            marks.psreadline_module_roots
        );
        assert_eq!(
            fs::read_to_string(root.join("7.ps1"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }

    #[test]
    fn shell_integration_unknown_record_never_changes_profile_or_record() {
        let root = super::super::tests::temp_dir("future-marks");
        let profile = root.join("profile.ps1");
        fs::write(&profile, LEGACY_LINE).unwrap();
        let mut future = serde_json::to_value(Marks::default()).unwrap();
        future["version"] = serde_json::json!(99);
        let mut unknown_field = serde_json::to_value(Marks::default()).unwrap();
        unknown_field["future"] = serde_json::json!(true);
        let mut relative = serde_json::to_value(Marks::default()).unwrap();
        relative["powershell_profiles"] = serde_json::json!(["relative.ps1"]);
        for record in [
            serde_json::to_vec(&future).unwrap(),
            b"not json".to_vec(),
            serde_json::to_vec(&unknown_field).unwrap(),
            serde_json::to_vec(&relative).unwrap(),
        ] {
            fs::write(root.join(RECORD_FILE), &record).unwrap();
            assert!(
                install_recorded(
                    &profile,
                    &root,
                    &root.join("folio.ps1"),
                    "new",
                    std::time::UNIX_EPOCH
                )
                .is_err()
            );
            assert_eq!(fs::read(&profile).unwrap(), LEGACY_LINE.as_bytes());
            assert_eq!(fs::read(root.join(RECORD_FILE)).unwrap(), record);
        }
    }

    /// PIN — **a holder that is not ours still gets the honest refusal, and it
    /// gets it only after [`OUR_TURN`].**
    ///
    /// This test was written to pin the refusal itself: a record under somebody
    /// else's lock is a record this run must not edit a `$PROFILE` against, and
    /// the file it guards has to be left byte-for-byte. All of that still
    /// stands. What changed on 2026-09-21 is *when* the refusal is honest. The
    /// refusal used to arrive the instant the lock was busy, which made every
    /// meeting between two of Folio's own writers a red toast on the reader's
    /// screen — see [`Asker`] — so an in-app writer now stands in the queue
    /// first. A holder that outlasts the queue is the case this pin is really
    /// about, and it is the case tested here: the wait runs out, nothing is
    /// written, and the refusal says exactly what it always said.
    ///
    /// Since 2026-09-23 a holder that went through [`lock`] is ours by
    /// construction and is waited for without a deadline, so the stranger
    /// here is a raw OS lock on a second handle to the lock file, which never
    /// stands in this process's queue. On both platforms a file lock held
    /// through another handle is exactly what another process looks like.
    #[test]
    fn shell_integration_record_lock_refuses_overlap_and_releases_on_drop() {
        let root = super::super::tests::temp_dir("marks-lock");
        let profile = root.join("profile.ps1");
        fs::write(&profile, LEGACY_LINE).unwrap();
        let held = foreign_holder(&root);
        let started = std::time::Instant::now();
        let refusal = install_recorded(
            &profile,
            &root,
            &script_at(&root),
            MANAGED_LINE,
            std::time::UNIX_EPOCH,
        )
        .expect_err("a foreign holder is a refusal, not a wait without end");
        assert!(
            started.elapsed() >= OUR_TURN,
            "the writer gave up before its turn was over: {:?}",
            started.elapsed()
        );
        assert!(
            refusal.to_string().contains("would block"),
            "the refusal lost its words: {refusal}"
        );
        assert_eq!(fs::read(&profile).unwrap(), LEGACY_LINE.as_bytes());
        drop(held);
        install_recorded(
            &profile,
            &root,
            &script_at(&root),
            MANAGED_LINE,
            std::time::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(fs::read(&profile).unwrap(), MANAGED_LINE.as_bytes());
    }

    /// PIN — **two of Folio's own writers that meet on the record both finish,
    /// and neither of them says anything to the reader.**
    ///
    /// These are the two that met on a clean Windows 10 machine on 2026-09-21,
    /// driven here exactly as the first-run card drives them: `Done` with the
    /// PowerShell row on spends [`crate::first_run::Application::PowerShellOffer`],
    /// which starts the enable worker ([`enable_record`]), and
    /// [`crate::first_run::Application::PowerShellIntent`], which the first
    /// PowerShell pane to name its `$PROFILE` spends through
    /// [`install_recorded`] on the window thread. The loser of that meeting
    /// reported *"lock acquisition failed because the operation would block"* in
    /// a red toast, on a machine where both rows had in fact been written.
    ///
    /// The third holder is what makes the meeting certain rather than likely:
    /// both writers are let go while the record is held, so both are queued
    /// behind it and then behind each other.
    ///
    /// MUTATIONS: make this process's queue refuse an `InApp` writer the way it
    /// refuses a door and both writers lose to the holder; drop either
    /// writer's mark from the record and the uninstall door has nothing to find.
    #[test]
    fn shell_integration_two_of_our_own_writers_queue_and_both_finish() {
        let root = super::super::tests::temp_dir("marks-queue");
        let profile = root.join("profile.ps1");
        fs::write(&profile, LEGACY_LINE).unwrap();
        let script = script_at(&root);
        let held = lock(&root, Asker::InApp).unwrap();
        let (install, enable) = std::thread::scope(|scope| {
            let installer = scope.spawn(|| {
                install_recorded(
                    &profile,
                    &root,
                    &script,
                    MANAGED_LINE,
                    std::time::UNIX_EPOCH,
                )
            });
            let enabler = scope.spawn(|| enable_record(&root));
            // Both writers are standing in the queue behind the holder (or
            // one has already left it, which the assertions below will name).
            while queue_watch::waiting(&root).len() < 2
                && !installer.is_finished()
                && !enabler.is_finished()
            {
                std::thread::yield_now();
            }
            drop(held);
            (installer.join().unwrap(), enabler.join().unwrap())
        });
        install.expect("the window thread's writer waited and wrote");
        enable.expect("the enable worker waited and wrote");
        assert_eq!(fs::read(&profile).unwrap(), MANAGED_LINE.as_bytes());
        let marks = Marks::read(&root).unwrap();
        assert!(!marks.is_off(), "the enable worker's mark");
        assert!(
            marks.powershell_profiles.contains(&profile)
                && marks.powershell_scripts.contains(&script),
            "the installer's marks: {marks:?}"
        );
        assert!(marks.profile_refusals.is_empty(), "{marks:?}");
    }

    /// PIN — **the first-run card's `Done` raises nothing**, with the two rows
    /// the clean-machine run had on.
    ///
    /// The card installs nothing itself; it spends
    /// [`crate::first_run::Application`]s, and this drives the two that reach
    /// the record — the Explorer row's does not — through the very functions
    /// `Done` reaches, against a temporary data root and a temporary
    /// `$PROFILE`. What the window would be told is a [`Report`], so the pin is
    /// that there is no report to tell: `begin_enable` builds one only out of
    /// an error, and [`Report::window_text`] is the whole of what a toast can
    /// say.
    #[test]
    fn shell_integration_first_run_done_tells_the_window_nothing() {
        let root = super::super::tests::temp_dir("first-run-done");
        let profile = root.join("profile.ps1");
        fs::write(&profile, b"# mine\r\n").unwrap();
        let script = script_at(&root);
        // The card the clean Windows 10 machine put up: three rows, no package
        // for Explorer's first page, no agent on the path — with the two rows
        // that arrived off switched on, as they were switched on there.
        let mut rows = crate::first_run::rows_for(
            bt_platform::HostPlatform::Windows,
            &crate::first_run::Machine {
                explorer_first_page_available: false,
                claude_found: false,
                claude_installable: false,
                codex_found: false,
                codex_installable: false,
                copilot_found: false,
                copilot_installable: false,
                powershell_integration_installed: false,
            },
        );
        for row in &mut rows {
            if matches!(
                row.kind,
                crate::first_run::RowKind::Explorer | crate::first_run::RowKind::PowerShell
            ) {
                row.on = true;
            }
        }
        let spent =
            crate::first_run::applications(&rows, crate::first_run::ExplorerShape::ClassicOnly);
        assert!(
            spent.contains(&crate::first_run::Application::PowerShellOffer(true))
                && spent.contains(&crate::first_run::Application::PowerShellIntent),
            "the card no longer spends the two answers this pin is about: {spent:?}"
        );
        let (install, enable) = std::thread::scope(|scope| {
            let installer = scope.spawn(|| {
                install_recorded(
                    &profile,
                    &root,
                    &script,
                    MANAGED_LINE,
                    std::time::UNIX_EPOCH,
                )
            });
            let enabler = scope.spawn(|| enable_record(&root));
            (installer.join().unwrap(), enabler.join().unwrap())
        });
        assert!(install.is_ok() && enable.is_ok(), "{install:?} {enable:?}");
        // What `begin_enable` would have put in front of the reader.
        let report = enable.err().map_or_else(Report::default, |error| Report {
            files: vec![FileReport {
                path: root.join(RECORD_FILE),
                fate: Fate::Refused(error.to_string()),
            }],
        });
        assert_eq!(report.window_text(), None, "the corner spoke");
        let written = fs::read_to_string(&profile).unwrap();
        assert!(
            written.starts_with("# mine\r\n") && written.contains(MANAGED_LINE),
            "{written:?}"
        );
    }

    /// A holder of the record that is not this process's: a raw OS lock taken
    /// through a second handle to the lock file, which never stands in
    /// [`profile_marks`]'s queue.
    fn foreign_holder(root: &Path) -> fs::File {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("integration-marks.lock"))
            .unwrap();
        file.try_lock().unwrap();
        file
    }

    /// RED (34) — **one of Folio's own writers waits behind another of ours for
    /// as long as that one takes, past any deadline.**
    ///
    /// Two CI runs on 2026-09-23 failed with `enable = Err("WouldBlock")`: the
    /// enable worker stood behind the installer, both of them Folio's, and the
    /// installer's fsync'd writes on a runner busy with four thousand other
    /// tests outlasted the two seconds [`OUR_TURN`] then gave every wait. The
    /// product rule is that two of our own writers both finish, and a reader's
    /// slow disk is the same machine as that runner. So the holder here is one
    /// of ours, taken through the product's own [`lock`], and it lets go only
    /// once the waiting [`enable_record`] has been standing in the queue for
    /// ten times its patience — observed, not slept on — and is still there.
    /// A waiter with a deadline has left long before that; one that stands
    /// for as long as ours takes has not. (Ten, not one: a deadline that has
    /// just passed and a waiter that has not yet woken to notice it look the
    /// same from outside.) The patience for this data root is cut to 50 ms so
    /// the test is quick; the product's own is untouched.
    ///
    /// MUTATION: give the wait in `OurTurn::take` a deadline of the asker's
    /// patience (`NEXT.wait_timeout`, then `WouldBlock`) and the waiter leaves
    /// before the holder does.
    #[test]
    fn our_own_writer_waits_behind_a_slow_holder_of_ours_past_any_deadline() {
        let root = super::super::tests::temp_dir("marks-slow-ours");
        let patience = std::time::Duration::from_millis(50);
        queue_watch::set_patience(&root, patience);
        let held = lock(&root, Asker::InApp).unwrap();
        let enable = std::thread::scope(|scope| {
            let waiter = scope.spawn(|| enable_record(&root));
            // Until the waiter has been in the queue for ten of its patiences,
            // or has left it.
            loop {
                if waiter.is_finished() {
                    break;
                }
                if queue_watch::waiting(&root)
                    .first()
                    .is_some_and(|joined| joined.elapsed() > patience * 10)
                {
                    break;
                }
                std::thread::yield_now();
            }
            assert!(
                !waiter.is_finished(),
                "the writer left the queue while ours was still ahead of it"
            );
            drop(held);
            waiter.join().unwrap()
        });
        enable.expect("our own writer waited its turn and wrote");
        assert!(!Marks::read(&root).unwrap().is_off());
    }

    /// PIN (34) — **a door still refuses at once, in the same words, when the
    /// holder is one of this process's own writers.**
    ///
    /// The queue without a deadline is for Folio's own writers; a door is a
    /// script with an exit code to read, and nothing about 2026-09-23 changes
    /// that. The door is asked while one of ours holds the record, and it
    /// must neither join the queue nor wait: it answers with the refusal the
    /// doors' transcripts have always carried, while the holder still holds.
    ///
    /// MUTATION: let a `Door` stand in `OurTurn::take`'s queue like `InApp`
    /// and it is seen waiting there.
    #[test]
    fn a_door_still_refuses_at_once() {
        let root = super::super::tests::temp_dir("marks-door-ours");
        let held = lock(&root, Asker::InApp).unwrap();
        let door = std::thread::scope(|scope| {
            let door = scope.spawn(|| lock(&root, Asker::Door).map(drop));
            while !door.is_finished() && queue_watch::waiting(&root).is_empty() {
                std::thread::yield_now();
            }
            let queued = !queue_watch::waiting(&root).is_empty();
            drop(held);
            let door = door.join().unwrap();
            assert!(!queued, "the door stood in our queue");
            door
        });
        let refusal = door.expect_err("a door refuses while ours holds the record");
        assert!(
            refusal.to_string().contains("would block"),
            "the refusal lost its words: {refusal}"
        );
    }

    #[test]
    fn shell_integration_migration_and_cleanup_retry_recorded_partial_refusal() {
        let root = super::super::tests::temp_dir("migration-refusal");
        let profiles = [root.join("redirected-5.1.ps1"), root.join("old-7.ps1")];
        for path in &profiles {
            fs::write(path, format!("# mine\r\n{LEGACY_LINE}\n\n")).unwrap();
        }
        let permissions = fs::metadata(&profiles[1]).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&profiles[1], readonly).unwrap();
        let original = fs::read(&profiles[1]).unwrap();
        let report = operate_with(
            &root,
            Asker::InApp,
            Action::Migrate,
            Ok(MANAGED_LINE),
            |_| (profiles.to_vec(), Report::default()),
        );
        assert_eq!(report.exit_code(), 1);
        assert_eq!(report.files[0].fate, Fate::Migrated);
        assert_eq!(fs::read(&profiles[1]).unwrap(), original);
        let marks = Marks::read(&root).unwrap();
        assert_eq!(marks.powershell_profiles, vec![profiles[0].clone()]);
        assert_eq!(marks.profile_refusals.len(), 1);
        assert_eq!(marks.profile_refusals[0].path, profiles[1]);
        fs::set_permissions(&profiles[1], permissions).unwrap();
        let report = operate_with(
            &root,
            Asker::InApp,
            Action::Remove,
            Ok(MANAGED_LINE),
            |marks| {
                let mut paths = marks.powershell_profiles.clone();
                paths.extend(marks.profile_refusals.iter().map(|r| r.path.clone()));
                (paths, Report::default())
            },
        );
        assert_eq!(report.exit_code(), 0);
        for path in &profiles {
            assert_eq!(fs::read(path).unwrap(), b"# mine\r\n\n");
        }
        assert!(Marks::read(&root).unwrap().profile_refusals.is_empty());
        let report = operate_with(
            &root,
            Asker::InApp,
            Action::Remove,
            Ok(MANAGED_LINE),
            |marks| (marks.powershell_profiles.clone(), Report::default()),
        );
        assert!(report.files.iter().all(|f| f.fate == Fate::Unchanged));
    }
}
