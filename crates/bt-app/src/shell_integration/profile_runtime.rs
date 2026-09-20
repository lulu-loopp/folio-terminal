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
                    let report = operate(&data, Action::Migrate);
                    // Script upkeep belongs to startup too, not the pane's offer read.
                    // Never writes from unit-test offer_for calls or a sandbox probe.
                    if std::env::var_os("BT_POWERSHELL_PROFILE").is_none() {
                        let _ = powershell_script_repaired();
                    }
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
    let mut paths = marks.powershell_profiles.clone();
    for refusal in &marks.profile_refusals {
        if !paths.contains(&refusal.path) {
            paths.push(refusal.path.clone());
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
pub fn remove_shell_integration() -> Report {
    operate(&persist::storage_dir(), Action::Remove)
}

fn operate(data: &Path, action: Action) -> Report {
    operate_with(data, action, candidates)
}

fn operate_with(
    data: &Path,
    action: Action,
    discover: impl FnOnce(&Marks) -> (Vec<PathBuf>, Report),
) -> Report {
    let record_path = data.join(RECORD_FILE);
    let refused_record = |error: io::Error| Report {
        files: vec![FileReport {
            path: record_path.clone(),
            fate: Fate::Refused(error.to_string()),
        }],
    };
    let _lock = match lock(data) {
        Ok(lock) => lock,
        Err(e) => return refused_record(e),
    };
    let mut marks = match Marks::read(data) {
        Ok(marks) => marks,
        Err(e) => return refused_record(e),
    };
    let (paths, mut report) = discover(&marks);
    let script = script_at(data);
    let mut scripts = marks.powershell_scripts.clone();
    if !scripts.contains(&script) {
        scripts.push(script.clone());
    }
    let forms = Forms::new(&scripts);

    // Write intent before changing somebody else's file: a crash can leave an
    // extra candidate, never an unrecorded installed mark. Removal retains the
    // locations too, so a later retry can revisit a locked/missing file.
    for path in &paths {
        marks.remember(path, &script);
    }
    if let Err(e) = marks.write(data) {
        return refused_record(e);
    }
    report.files.extend(apply(&paths, &forms, action).files);
    // Probe refusals name executables, not profiles. They are reported on this
    // run, and retried through discovery, never treated as profile candidates.
    marks.profile_refusals = report
        .refusals()
        .into_iter()
        .filter(|r| paths.contains(&r.path))
        .collect();
    if let Err(e) = marks.write(data) {
        report.files.extend(refused_record(e).files);
    }
    report
}

/// Install's record transaction. Unit tests inject both profile and data root.
pub fn install_recorded(
    profile: &Path,
    data: &Path,
    script: &Path,
    line: &str,
    at: std::time::SystemTime,
) -> io::Result<ProfileWrite> {
    let profile = std::path::absolute(profile)?;
    let script = std::path::absolute(script)?;
    let _lock = lock(data)?;
    let mut marks = Marks::read(data)?;
    marks.remember(&profile, &script);
    marks.write(data)?;
    let result = add_profile_with_forms(&profile, line, &Forms::new(&marks.powershell_scripts), at);
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

pub fn begin_removal() {
    let _ = bt_platform::spawn_at_priority(
        "powershell-profile-removal",
        bt_platform::ThreadPriority::BelowNormal,
        || {
            let report = remove_shell_integration();
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

    #[test]
    fn shell_integration_record_lock_refuses_overlap_and_releases_on_drop() {
        let root = super::super::tests::temp_dir("marks-lock");
        let profile = root.join("profile.ps1");
        fs::write(&profile, LEGACY_LINE).unwrap();
        let held = lock(&root).unwrap();
        assert!(
            install_recorded(
                &profile,
                &root,
                &script_at(&root),
                MANAGED_LINE,
                std::time::UNIX_EPOCH
            )
            .is_err()
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
        let report = operate_with(&root, Action::Migrate, |_| {
            (profiles.to_vec(), Report::default())
        });
        assert_eq!(report.exit_code(), 1);
        assert_eq!(report.files[0].fate, Fate::Migrated);
        assert_eq!(fs::read(&profiles[1]).unwrap(), original);
        let marks = Marks::read(&root).unwrap();
        assert_eq!(marks.powershell_profiles, profiles);
        assert_eq!(marks.profile_refusals.len(), 1);
        assert_eq!(marks.profile_refusals[0].path, profiles[1]);
        fs::set_permissions(&profiles[1], permissions).unwrap();
        let report = operate_with(&root, Action::Remove, |marks| {
            (marks.powershell_profiles.clone(), Report::default())
        });
        assert_eq!(report.exit_code(), 0);
        for path in &profiles {
            assert_eq!(fs::read(path).unwrap(), b"# mine\r\n\n");
        }
        assert!(Marks::read(&root).unwrap().profile_refusals.is_empty());
        let report = operate_with(&root, Action::Remove, |marks| {
            (marks.powershell_profiles.clone(), Report::default())
        });
        assert!(report.files.iter().all(|f| f.fate == Fate::Unchanged));
    }
}
