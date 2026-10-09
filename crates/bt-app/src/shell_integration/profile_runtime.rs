//! Discovery, startup scheduling and the shared non-interactive removal door.
use super::{profile_marks::*, *};
use crate::i18n::Text;
use std::{io, sync::Mutex};

static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static REMOVAL: Mutex<Option<Report>> = Mutex::new(None);
/// Every answered one-click install or Undo not yet delivered, each with the window that asked.
/// A queue, not a slot: two windows' clicks in flight are two answers, and neither replaces the
/// other.
static PROFILE_INSTALLS: Mutex<Vec<ProfileInstallAnswer>> = Mutex::new(Vec::new());

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileInstallOutcome {
    /// The line was written; `edit` is what its Undo takes back.
    Installed {
        program: PathBuf,
        edit: ProfileEdit,
    },
    /// The file already carried a line in a form Folio owns, read under the lock immediately
    /// before the edit: nothing was written, and there is nothing to undo.
    Present,
    Refused(String),
    Undone,
    UndoRefused(String),
}

/// One answered request: **the window is part of the address** (`files::DirRequest`'s rule), so
/// the outcome and the Undo handle it carries reach the window whose click asked for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileInstallAnswer {
    pub window: winit::window::WindowId,
    pub outcome: ProfileInstallOutcome,
}

fn answer_profile_install(window: winit::window::WindowId, outcome: ProfileInstallOutcome) {
    if let Ok(mut answers) = PROFILE_INSTALLS.lock() {
        answers.push(ProfileInstallAnswer { window, outcome });
    }
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

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

/// No profile read, process query, script read or script write happens on the caller, and the
/// caller waits for nothing: one worker observes whether an older Folio line is present, without
/// editing a profile, and wakes the window when it knows. Folio's own script is prepared elsewhere
/// (`shell_integration::begin_powershell_script_preparation` at launch, and on each PowerShell
/// birth's worker), so this observation never stands between a window and its first pane.
///
/// **Held back in an update's trial** behind [`crate::update_trial::Writer::ProfileMigration`], as
/// the migration it replaced was. The observation itself only reads; the commit's release calls
/// this again, so the conditional Settings verb appears then.
pub fn begin_startup_migration() {
    if crate::update_trial::defer(crate::update_trial::Writer::ProfileMigration) {
        return;
    }
    if STARTED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    spawn_profile_observation(persist::storage_dir(), Vec::new(), Vec::new());
}

/// Refresh the two edition facts when the Profiles page opens. This is an edge
/// trigger supplied by Settings, not a polling loop; a second visit asks again —
/// the observation, a failed `$PROFILE` path, and every parse question that got
/// no answer (release read m2).
pub fn begin_profile_observation_for(programs: &profiles::ProfilePrograms) {
    let mut resolved = Vec::new();
    for profile in profiles::table().profiles() {
        let Some(program) = programs.program(&profile.id).map(PathBuf::from) else {
            continue;
        };
        if is_powershell(&program) && !resolved.contains(&program) {
            resolved.push(program);
        }
    }
    spawn_profile_observation(
        persist::storage_dir(),
        resolved,
        parse_questions(profiles::table().profiles(), programs),
    );
}

/// **The observation's worker**: the installed PowerShells — looked up in the environment it reads,
/// so on the worker and never on the window thread — and `rows` (the profile rows' own programs,
/// as the program walk answered them), each observed; then the parse questions that got no answer.
fn spawn_profile_observation(data: PathBuf, rows: Vec<PathBuf>, questions: Vec<ParseQuestion>) {
    let _ = bt_platform::spawn_at_priority(
        "powershell-profile-observation",
        bt_platform::ThreadPriority::BelowNormal,
        move |worker| {
            let environment = ProbeEnvironment::current(worker, "PowerShell profile observation");
            let mut programs = installed_powershells(&environment);
            for program in rows {
                if !programs.contains(&program) {
                    programs.push(program);
                }
            }
            let report = observe_profile_lines(&data, &programs, &environment);
            ask_failed_parse_questions_again(questions, &environment);
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
}

/// **Whether the Settings remover has anything to remove here**: a line in one of the exact
/// forms the removal owns ([`Forms::owns`]) — never the looser test above, which also
/// recognises a line somebody wrote by hand and the remover cannot prove is Folio's (census
/// item 5: the verb stood on screen and did nothing). A hand-written line still integrates the
/// edition, and its row says so; it offers no verb.
fn profile_carries_an_owned_line(path: &Path, forms: &Forms) -> bool {
    bt_platform::file_reads::read(bt_platform::file_reads::Lane::Settings, path).is_ok_and(
        |bytes| {
            Decoded::read(&bytes)
                .is_ok_and(|decoded| decoded.text.lines().any(|line| forms.owns(line)))
        },
    )
}

/// The forms a removal owns, for the record `marks` and the data root `data`.
fn owned_forms(marks: &Marks, data: &Path) -> Forms {
    let mut scripts = marks.powershell_scripts.clone();
    let script = script_at(data);
    if !scripts.contains(&script) {
        scripts.push(script);
    }
    Forms::new(&scripts)
}

/// Read-only startup discovery for the conditional Settings remover. Legacy marks remain useful
/// as candidate locations, but no migration is allowed to rewrite a profile now that integration
/// is process-scoped.
///
/// The observations run first: each one that answers files its `$PROFILE` path as the newest
/// answer for its program ([`file_profile_answer`]), so the removal's path question below is
/// asked only of an edition that did not answer.
fn observe_profile_lines(
    data: &Path,
    programs: &[PathBuf],
    environment: &ProbeEnvironment,
) -> Report {
    // Numbered as it starts: a run that overlaps a later one never files over it.
    let observation = next_observation();
    if !profile_sandboxed() {
        for program in programs {
            match probe_profile_observation(program, environment) {
                Some(mut observed) => {
                    observed.seen = ProfileRevision::read(&observed.path);
                    file_profile_answer(program, observed.path.clone(), observation, environment);
                    publish_profile_observation(program, observation, observed);
                }
                None => publish_profile_observation_failed(program, observation),
            }
        }
    }
    let answers = profile_answers(data, POWERSHELL_PROBE_PATIENCE, environment);
    let (marks, files) = match Marks::read(data).and_then(|marks| {
        let files = ProfileFiles::read(data)?;
        Ok((marks, files))
    }) {
        Ok(read) => read,
        Err(error) => {
            publish_powershell_profile_line_present(false);
            return Report {
                files: vec![FileReport {
                    path: data.join(RECORD_FILE),
                    fate: Fate::Refused(error.to_string()),
                }],
            };
        }
    };
    let forms = owned_forms(&marks, data);
    let (paths, report) = candidates(&marks, &files, answers);
    publish_powershell_profile_line_present(
        paths
            .iter()
            .any(|path| profile_carries_an_owned_line(path, &forms)),
    );
    report
}

/// What each installed PowerShell answered about where its `$PROFILE` is: `None` for one that
/// did not answer.
type PathAnswers = Vec<(PathBuf, Option<PathBuf>)>;

/// Every `$PROFILE` a removal must look at: the sandbox's alone when it is set; otherwise every
/// recorded path ([`Marks`], [`ProfileFiles`]) and every path an edition named in `answers`.
/// An edition that did not answer is reported [`Fate::Unlocated`] — not a refusal: what Folio
/// recorded is removed whatever any shell says (release read M1).
fn candidates(marks: &Marks, files: &ProfileFiles, answers: PathAnswers) -> (Vec<PathBuf>, Report) {
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
        .chain(files.profiles.iter().map(|entry| &entry.profile))
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
    for (program, answer) in answers {
        match answer {
            Some(path) => {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            None => report.files.push(FileReport {
                path: program,
                fate: Fate::Unlocated,
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
///
/// `environment` is where its PowerShells are looked up and asked: the current logon block on the
/// Settings remover's worker, this process's own for the command-line verb.
pub fn remove_shell_integration(asker: Asker, environment: &ProbeEnvironment) -> Report {
    operate(&persist::storage_dir(), asker, Action::Remove, environment)
}

/// The cleanup door supplies its resolved data root without triggering storage migration.
/// An explicit profile set replaces historical and probed paths, just like the sandbox env door.
pub fn remove_shell_integration_at(data: &Path, profiles: Option<&[PathBuf]>) -> Report {
    // A door, by the name on it: this is `--uninstall-cleanup`'s road, and it has
    // already refused if a Folio is running. Only a run that will ask the machine
    // where its profiles are pays for asking.
    let answers = match profiles {
        None => profile_answers(data, REMOVAL_PROBE_PATIENCE, &ProbeEnvironment::Inherited),
        Some(_) => Vec::new(),
    };
    operate_with(
        data,
        Asker::Door,
        Action::Remove,
        Ok(MANAGED_LINE),
        |marks, files| {
            profiles.map_or_else(
                || candidates(marks, files, answers),
                |paths| (paths.to_vec(), Report::default()),
            )
        },
    )
}

/// **Ask the machine its slow question before the record is locked** — where each installed
/// PowerShell keeps `$PROFILE`, for every edition the record does not already locate.
///
/// The answer costs a child process apiece. Asked where it used to be asked — inside
/// [`operate_with`], under the lock — one of Folio's own writers could hold the record while
/// shells started, which is the difference between a turn worth waiting for and a wait nobody can
/// be asked to make on a window thread. An edition the record locates
/// ([`ProfileFiles::located`], written when Folio wrote its line) is not asked: the probe is the
/// fallback for a line an older Folio wrote without that record (release read M1). `patience` is
/// the asker's ([`POWERSHELL_PROBE_PATIENCE`], [`REMOVAL_PROBE_PATIENCE`]). The record is read here without
/// the lock — to decide only which editions to ask; the removal reads it again under the lock.
fn profile_answers(
    data: &Path,
    patience: bt_platform::ProbePatience,
    environment: &ProbeEnvironment,
) -> PathAnswers {
    // The sandbox door replaces the whole candidate set, so no shell is asked.
    if profile_sandboxed() {
        return Vec::new();
    }
    answers_for(
        &ProfileFiles::read(data).unwrap_or_default(),
        installed_powershells(environment),
        |program| cached_profile_answer(program, environment, ProfileQuestion::Ask(patience)),
    )
}

/// [`profile_answers`]'s one decision: ask each program whose edition the record does not
/// locate, and only those.
fn answers_for(
    files: &ProfileFiles,
    programs: Vec<PathBuf>,
    mut ask: impl FnMut(&Path) -> Option<PathBuf>,
) -> PathAnswers {
    programs
        .into_iter()
        .filter(|program| files.located(powershell_edition(program)).is_none())
        .map(|program| {
            let answer = ask(&program);
            (program, answer)
        })
        .collect()
}

fn operate(data: &Path, asker: Asker, action: Action, environment: &ProbeEnvironment) -> Report {
    let answers = profile_answers(data, REMOVAL_PROBE_PATIENCE, environment);
    operate_with(data, asker, action, Ok(MANAGED_LINE), |marks, files| {
        candidates(marks, files, answers)
    })
}

fn operate_with(
    data: &Path,
    asker: Asker,
    action: Action,
    managed: io::Result<&'static str>,
    discover: impl FnOnce(&Marks, &ProfileFiles) -> (Vec<PathBuf>, Report),
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
    let mut files = match ProfileFiles::read(data) {
        Ok(files) => files,
        Err(e) => return refused_record(e),
    };
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
    let (paths, mut report) = discover(&marks, &files);
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
        apply_recorded(&paths, &forms, action, &mut files, |path| {
            marks.remember(path, &script);
            if records { marks.write(data) } else { Ok(()) }
        })
        .files,
    );
    if action != Action::Remove || asker == Asker::InApp {
        publish_powershell_profile_line_present(
            paths
                .iter()
                .any(|path| profile_carries_an_owned_line(path, &forms)),
        );
    }
    // Probe refusals name executables, not profiles. They are reported on this
    // run, and retried through discovery, never treated as profile candidates.
    marks.profile_refusals = report
        .refusals()
        .into_iter()
        .filter(|r| paths.contains(&r.path))
        .collect();
    // Merely discovering a hand-written installation must not create an
    // enabled record. Existing records and explicit Off decisions still persist.
    // What the removal retired is written back only where its record already is.
    let written = [
        (records && (record_path.exists() || !marks.profile_refusals.is_empty()))
            .then(|| marks.write(data)),
        (records && data.join(FILES_RECORD).exists()).then(|| files.write(data)),
    ];
    for error in written.into_iter().flatten().filter_map(Result::err) {
        report.files.extend(refused_record(error).files);
    }
    report
}

/// Install's record transaction. Unit tests inject both profile and data root.
#[cfg(test)]
fn enable_record(data: &Path) -> io::Result<()> {
    let _lock = lock(data, Asker::InApp)?;
    let mut marks = Marks::read(data)?;
    marks.powershell_state = PowerShellState::Enabled {};
    marks.write(data)
}

/// **Write the managed line, recording first what the write brings into existence**
/// ([`ProfileFiles::before_write`]): the file and its folders when there is none, or the one copy
/// of a file Folio has not written into before. `edition` is the edition that named `profile`,
/// recorded so a removal finds the file without asking it again; `None` where the caller does
/// not know (a test, a fixture). A write that fails takes the record back to what it was
/// ([`ProfileFiles::write_failed`]).
///
/// **Made against `seen`** (0.4.8 G7): the profile as the check the person answered read it.
/// Under the lock, before anything is recorded, the file is read again; bytes that differ are
/// refused with [`Text::ShellProfileChangedElsewhere`] and nothing is written — a check that saw
/// no line while somebody, or another window, has written one since is never answered "Added".
pub fn install_recorded(
    profile: &Path,
    data: &Path,
    script: &Path,
    line: &'static str,
    at: std::time::SystemTime,
    edition: Option<PowerShellEdition>,
    seen: &ProfileRevision,
) -> io::Result<ProfileWrite> {
    let profile = std::path::absolute(profile)?;
    let script = std::path::absolute(script)?;
    let _lock = lock(data, Asker::InApp)?;
    if let Some(error) = seen.unreadable() {
        crate::diagnostics::note(&format!(
            "BT_SHELL_PROFILE {}: the check could not read it ({error}); Enable refused",
            profile.display()
        ));
        return Err(io::Error::other(Text::ShellProfileChangedElsewhere.text()));
    }
    if ProfileRevision::of_edit(&profile)? != *seen {
        return Err(io::Error::other(Text::ShellProfileChangedElsewhere.text()));
    }
    let mut marks = Marks::read(data)?;
    let mut files = ProfileFiles::read(data)?;
    marks.powershell_state = PowerShellState::Enabled {};
    marks.remember(&profile, &script);
    marks.write(data)?;
    let previous = files.entry(&profile).cloned();
    let backup = files.before_write(&profile, edition, at);
    files.write(data)?;
    let result = add_profile_with_forms(
        &profile,
        line,
        &Forms::new(&marks.powershell_scripts).targeting(line),
        backup.as_deref(),
    );
    marks.profile_refusals.retain(|r| r.path != profile);
    if let Err(e) = &result {
        files.write_failed(previous, &profile);
        files.write(data)?;
        marks.profile_refusals.push(Refusal {
            path: profile,
            reason: e.to_string(),
        });
    }
    marks.write(data)?;
    result
}

/// The click's own re-probe: the policy is asked again — for an ordinary session of the edition
/// and for the row that was clicked, its own `-ExecutionPolicy` included ([`edition_cause`]) — and
/// a cause that would keep `$PROFILE` from loading in either is refused with the sentence the row
/// would show.
///
/// `seen` is what the row's check read ([`super::powershell_profile_seen`]); `None` when no check
/// had answered, which is refused as a profile that changed elsewhere would be — there is no
/// revision to write against.
fn install_for_program(
    worker: &bt_platform::admission::WorkerCtx,
    program: &Path,
    arguments: &[OsString],
    seen: Option<&ProfileRevision>,
) -> io::Result<Option<ProfileEdit>> {
    let Some(seen) = seen else {
        return Err(io::Error::other(Text::ShellProfileChangedElsewhere.text()));
    };
    let environment = ProbeEnvironment::current(worker, "PowerShell profile install");
    let observation = next_observation();
    let Some(mut observed) = probe_profile_observation(program, &environment) else {
        publish_profile_observation_failed(program, observation);
        return Err(io::Error::other(Text::ShellProfileProbeFailed.text()));
    };
    observed.seen = ProfileRevision::read(&observed.path);
    file_profile_answer(program, observed.path.clone(), observation, &environment);
    publish_profile_observation(program, observation, observed.clone());
    // A line in a Constrained Language Mode session loads the script into that mode, where it
    // does nothing: refused with the row's own sentence.
    if !observed.full_language {
        return Err(io::Error::other(Text::CapPowerShellConstrained.text()));
    }
    if let Some(sentence) =
        edition_cause(&observed, row_process_scope(program, arguments)).sentence()
    {
        return Err(io::Error::other(sentence.text()));
    }
    let data = persist::storage_dir();
    let script = install_script_at(&data.join(SCRIPT_DIRECTORY), SCRIPT_FILE_PS1, SCRIPT_PS1)
        .map_err(|_| io::Error::other(Text::ShellProfileRefused.text()))?;
    let wrote = install_recorded(
        &observed.path,
        &data,
        &script,
        MANAGED_LINE,
        std::time::SystemTime::now(),
        Some(powershell_edition(program)),
        seen,
    )?;
    observed.seen = ProfileRevision::read(&observed.path);
    publish_profile_observation(program, observation, observed.clone());
    publish_powershell_profile_line_present(true);
    Ok(wrote.edit)
}

/// **Undo of one click**: the click's edit taken back, then what its write brought into existence
/// retired ([`ProfileFiles::retire`]) — the file and its folders when Folio created them and
/// nothing else is in the file, and the copy. Under the marks lock, like the write.
///
/// **Only while the file is exactly what the click wrote** (0.4.8 G7): it is read again under the
/// lock and refused with [`Text::ShellProfileChangedElsewhere`] when its bytes are not the edit's
/// `after`. Then it is put back to the edit's `before` — the click's line out, with the separator
/// it added, byte for byte — or emptied where there was no file, for the retirement to delete. A
/// line the person wrote, or Folio's line once they have edited it, is never taken out by Undo;
/// the Settings remover still removes a line in a form Folio owns.
pub(super) fn undo_profile_install(edit: &ProfileEdit, data: &Path) -> io::Result<()> {
    let _lock = lock(data, Asker::InApp)?;
    let mut files = ProfileFiles::read(data)?;
    let profile = edit.profile.as_path();
    if ProfileRevision::of_edit(profile)? != edit.after {
        return Err(io::Error::other(Text::ShellProfileChangedElsewhere.text()));
    }
    replace_profile(profile, edit.after.bytes(), edit.before.bytes(), None)?;
    let forms = Forms::new(&[]).targeting(MANAGED_LINE);
    files.retire(profile, &forms)?;
    if data.join(FILES_RECORD).exists() || !files.profiles.is_empty() {
        files.write(data)?;
    }
    Ok(())
}

pub fn begin_profile_install(
    program: PathBuf,
    arguments: Vec<OsString>,
    seen: Option<ProfileRevision>,
    window: winit::window::WindowId,
) {
    let _ = bt_platform::spawn_at_priority(
        "powershell-profile-install",
        bt_platform::ThreadPriority::BelowNormal,
        move |worker| {
            let outcome = match install_for_program(worker, &program, &arguments, seen.as_ref()) {
                Ok(Some(edit)) => ProfileInstallOutcome::Installed { program, edit },
                Ok(None) => ProfileInstallOutcome::Present,
                Err(error) => ProfileInstallOutcome::Refused(error.to_string()),
            };
            answer_profile_install(window, outcome);
        },
    );
}

pub fn begin_profile_install_undo(
    program: PathBuf,
    edit: ProfileEdit,
    window: winit::window::WindowId,
) {
    let _ = bt_platform::spawn_at_priority(
        "powershell-profile-install-undo",
        bt_platform::ThreadPriority::BelowNormal,
        move |worker| {
            let data = persist::storage_dir();
            let outcome = match undo_profile_install(&edit, &data) {
                Ok(()) => {
                    let environment =
                        ProbeEnvironment::current(worker, "PowerShell profile observation");
                    let _ = observe_profile_lines(&data, &[program], &environment);
                    ProfileInstallOutcome::Undone
                }
                Err(error) => ProfileInstallOutcome::UndoRefused(error.to_string()),
            };
            answer_profile_install(window, outcome);
        },
    );
}

/// **The answers addressed to `window`**, oldest first — what the window owed them delivers; an
/// answer for a window that has since closed is delivered nowhere.
pub fn profile_installs_for(
    answers: &[ProfileInstallAnswer],
    window: winit::window::WindowId,
) -> impl Iterator<Item = &ProfileInstallOutcome> {
    answers
        .iter()
        .filter(move |answer| answer.window == window)
        .map(|answer| &answer.outcome)
}

/// Every answer that has arrived since the last call, oldest first.
pub fn take_profile_installs() -> Vec<ProfileInstallAnswer> {
    PROFILE_INSTALLS
        .lock()
        .map(|mut answers| std::mem::take(&mut *answers))
        .unwrap_or_default()
}

pub fn begin_removal() {
    let _ = bt_platform::spawn_at_priority(
        "powershell-profile-removal",
        bt_platform::ThreadPriority::BelowNormal,
        |worker| {
            let environment = ProbeEnvironment::current(worker, "PowerShell profile removal");
            let report = remove_shell_integration(Asker::InApp, &environment);
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
    /// given the words `No Folio profile lines found.`. The explicit Settings
    /// cleanup action must remain silent when there was no legacy line.
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

    /// RED (mutations: bypass `install_recorded`, omit the managed line, or
    /// make Undo a no-op) — the click and its toast verb use the existing
    /// atomic profile writer against an injected location only.
    #[test]
    fn inject4_profile_fallback_click_and_undo_use_the_managed_writer_in_a_sandbox() {
        let root = super::super::tests::temp_dir("profile-fallback-click");
        let data = root.join("data");
        let profile = root.join("PowerShell").join("profile.ps1");
        let script = data.join(SCRIPT_DIRECTORY).join(SCRIPT_FILE_PS1);
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, SCRIPT_PS1).unwrap();

        let wrote = install_recorded(
            &profile,
            &data,
            &script,
            MANAGED_LINE,
            std::time::UNIX_EPOCH,
            None,
            &ProfileRevision::read(&profile),
        )
        .expect("one click writes the managed line");
        assert_eq!(wrote.profile, std::path::absolute(&profile).unwrap());
        assert_eq!(
            fs::read_to_string(&profile).unwrap(),
            format!("{MANAGED_LINE}\r\n")
        );
        assert!(
            Marks::read(&data)
                .unwrap()
                .powershell_profiles
                .contains(&wrote.profile)
        );

        undo_profile_install(edit(&wrote), &data).expect("Undo removes the managed line");
        assert!(
            !profile.exists() && !profile.parent().unwrap().exists(),
            "the file and the folder this click created are gone with its line"
        );
    }

    /// Every file a directory holds, by name, sorted.
    fn names(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// The edit a click's write made — the handle its Undo takes back.
    fn edit(write: &ProfileWrite) -> &ProfileEdit {
        write.edit.as_ref().expect("the click wrote its line")
    }

    /// **The Settings remover's road** — `operate_with` removing every line in a form Folio
    /// owns from every recorded profile, and retiring what Folio's writes created — the next
    /// removal after a write that never happened, or after an Undo that was refused.
    fn remove_every_line(data: &Path) -> Report {
        operate_with(
            data,
            Asker::InApp,
            Action::Remove,
            Ok(MANAGED_LINE),
            |marks, files| candidates(marks, files, Vec::new()),
        )
    }

    /// One install of the managed line into `profile`, recorded for Windows PowerShell.
    fn enable(profile: &Path, data: &Path) -> ProfileWrite {
        let script = data.join(SCRIPT_DIRECTORY).join(SCRIPT_FILE_PS1);
        install_recorded(
            profile,
            data,
            &script,
            MANAGED_LINE,
            std::time::UNIX_EPOCH,
            Some(PowerShellEdition::WindowsPowerShell),
            &ProfileRevision::read(profile),
        )
        .expect("the managed line is written")
    }

    /// PIN (release read B1, M5) — **what Folio's writes into `$PROFILE` brought into existence
    /// goes with its line, and nothing else does.** One case per kind of file Folio can meet:
    ///
    /// * no file and no folder: Enable creates both, recording them first; Undo takes the line
    ///   out, then the empty file, then the folder — an empty `$PROFILE` is itself refused under
    ///   `Restricted`, so leaving it would leave every session's error;
    /// * a file of the person's own: one copy, taken before Folio's first write and not again on a
    ///   second; the line's removal leaves the file and deletes the copy;
    /// * a file Folio created that the person has since added to: it stays, folder and all;
    /// * an empty file the person had: it stays — Folio did not create it;
    /// * a folder that was there, with no file: the file goes, the folder stays.
    ///
    /// Every case leaves no `.bak-` copy behind, and the record keeps only the edition's location.
    ///
    /// RED (mutations: `retire_keeps_file` — `retire` skips the file's deletion: the first case
    /// keeps an empty `$PROFILE`; `whitespace_any` — the content test dropped: the third case
    /// deletes the person's line; `created_ignored` — `created_file` not consulted: the fourth
    /// case deletes the person's empty file; `backup_every_write` — `before_write` takes a copy on
    /// every write: two copies; `backup_kept` — `retire` leaves the copy).
    #[test]
    fn what_folios_profile_writes_created_goes_with_its_line_and_nothing_else_does() {
        let root = super::super::tests::temp_dir("profile-created-files");
        let data = root.join("data");
        let edition = PowerShellEdition::WindowsPowerShell;

        // No file, no folder.
        let created = root
            .join("文档 one")
            .join("WindowsPowerShell")
            .join("profile.ps1");
        let wrote = enable(&created, &data);
        let entry = ProfileFiles::read(&data)
            .unwrap()
            .entry(&created)
            .cloned()
            .unwrap();
        assert!(entry.created_file);
        assert_eq!(
            entry.created_folders,
            [
                root.join("文档 one"),
                root.join("文档 one").join("WindowsPowerShell")
            ]
        );
        assert_eq!(entry.backup, None, "there was nothing to copy");
        undo_profile_install(edit(&wrote), &data).unwrap();
        assert!(!created.exists(), "the empty file Folio created is gone");
        assert!(
            !root.join("文档 one").exists(),
            "and both folders it created"
        );
        let files = ProfileFiles::read(&data).unwrap();
        assert_eq!(files.located(edition), Some(created.as_path()));
        let entry = files.entry(&created).unwrap();
        assert!(!entry.created_file && entry.created_folders.is_empty() && entry.backup.is_none());

        // A file of the person's own.
        let theirs = root.join("theirs").join("profile.ps1");
        fs::create_dir_all(theirs.parent().unwrap()).unwrap();
        let mine = "Set-Location D:\\项目\r\n";
        fs::write(&theirs, mine).unwrap();
        let first = enable(&theirs, &data);
        let copy = first
            .backup
            .clone()
            .expect("one copy before the first write");
        assert_eq!(fs::read_to_string(&copy).unwrap(), mine);
        assert_eq!(enable(&theirs, &data).backup, None, "no second copy");
        assert_eq!(
            names(theirs.parent().unwrap()).len(),
            2,
            "the file and its one copy"
        );
        undo_profile_install(edit(&first), &data).unwrap();
        assert!(
            fs::read_to_string(&theirs).unwrap().starts_with(mine),
            "the person's file stays"
        );
        assert_eq!(
            names(theirs.parent().unwrap()),
            ["profile.ps1"],
            "the copy is gone"
        );

        // A file Folio created, added to since: its Undo is refused, for the file is not what
        // the click wrote (0.4.8 G7), and the Settings remover takes the line out and keeps the
        // person's.
        let added = root.join("added").join("profile.ps1");
        let wrote = enable(&added, &data);
        let with_line = fs::read_to_string(&added).unwrap();
        let theirs_too = format!("{with_line}Write-Host 'mine 中'\r\n");
        fs::write(&added, &theirs_too).unwrap();
        assert!(undo_profile_install(edit(&wrote), &data).is_err());
        assert_eq!(fs::read_to_string(&added).unwrap(), theirs_too);
        remove_every_line(&data);
        assert_eq!(
            fs::read_to_string(&added).unwrap(),
            "Write-Host 'mine 中'\r\n"
        );

        // An empty file the person had.
        let empty = root.join("empty").join("profile.ps1");
        fs::create_dir_all(empty.parent().unwrap()).unwrap();
        fs::write(&empty, b"").unwrap();
        let wrote = enable(&empty, &data);
        undo_profile_install(edit(&wrote), &data).unwrap();
        assert_eq!(fs::read(&empty).unwrap(), b"", "Folio did not create it");
        assert_eq!(names(empty.parent().unwrap()), ["profile.ps1"]);

        // A folder that was there, with no file.
        let folder = root.join("folder");
        fs::create_dir_all(&folder).unwrap();
        let in_folder = folder.join("profile.ps1");
        let wrote = enable(&in_folder, &data);
        assert!(
            ProfileFiles::read(&data)
                .unwrap()
                .entry(&in_folder)
                .unwrap()
                .created_folders
                .is_empty()
        );
        undo_profile_install(edit(&wrote), &data).unwrap();
        assert!(!in_folder.exists() && folder.is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    /// PIN (census item 5) — **the Settings remover stands only for a line it can prove is
    /// Folio's.** A hand-written line that dot-sources `folio.ps1` still integrates the edition
    /// (the observation's "present", which keeps Enable away), but no exact form the removal owns
    /// is in it, so the remover row is not offered — it would press and remove nothing. Each
    /// managed and literal legacy form is offered.
    ///
    /// RED (mutation: `loose_remover` — the remover's presence reads
    /// `ProfileRevision::carries_the_line`, as it did: the hand-written line shows a verb that
    /// does nothing).
    #[test]
    fn the_remover_stands_only_for_a_line_it_can_prove_is_folios() {
        let root = super::super::tests::temp_dir("profile-owned-line");
        let data = root.join("data");
        let forms = owned_forms(&Marks::default(), &data);
        let profile = root.join("profile.ps1");
        for (text, integrates, removable) in [
            (". 'D:\\工具\\folio.ps1'\r\n", true, false),
            (
                "# . \"$env:APPDATA\\Folio\\shell-integration\\folio.ps1\"\r\n",
                false,
                false,
            ),
            (format!("# mine\r\n{MANAGED_LINE}\r\n").as_str(), true, true),
            (format!("{LEGACY_LINE}\n").as_str(), true, true),
            (
                format!(". \"{}\"\r\n", script_at(&data).display()).as_str(),
                true,
                true,
            ),
        ] {
            fs::write(&profile, text).unwrap();
            assert_eq!(
                ProfileRevision::read(&profile).carries_the_line(),
                integrates,
                "{text}"
            );
            assert_eq!(
                profile_carries_an_owned_line(&profile, &forms),
                removable,
                "{text}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// PIN (release read B1, M5) — **every removal road retires the same way as Undo**: the
    /// Settings remover, `--remove-shell-integration` and both uninstall verbs go through
    /// `operate_with`, which retires each profile whose line it took out (or found absent), and
    /// never one whose removal was refused.
    ///
    /// RED (mutations: `apply_skips_retire` — `apply_recorded` does not retire: the created file
    /// stays; `retire_on_refusal` — it retires after a refused removal too: the copy of the
    /// read-only file is deleted while its line stays).
    #[test]
    fn every_removal_road_retires_what_the_write_created() {
        let root = super::super::tests::temp_dir("profile-removal-retires");
        let data = root.join("data");
        let created = root.join("PowerShell").join("profile.ps1");
        enable(&created, &data);
        let locked = root.join("locked").join("profile.ps1");
        fs::create_dir_all(locked.parent().unwrap()).unwrap();
        fs::write(&locked, "# 我的\r\n").unwrap();
        let copy = enable(&locked, &data).backup.unwrap();
        let permissions = fs::metadata(&locked).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&locked, readonly).unwrap();
        let report = operate_with(
            &data,
            Asker::InApp,
            Action::Remove,
            Ok(MANAGED_LINE),
            |marks, files| candidates(marks, files, Vec::new()),
        );
        assert_eq!(report.exit_code(), 1, "the read-only file refuses");
        assert!(!created.exists() && !root.join("PowerShell").exists());
        assert!(copy.exists(), "a refused removal keeps its copy");
        fs::set_permissions(&locked, permissions).unwrap();
        let report = operate_with(
            &data,
            Asker::InApp,
            Action::Remove,
            Ok(MANAGED_LINE),
            |marks, files| candidates(marks, files, Vec::new()),
        );
        assert_eq!(report.exit_code(), 0, "{}", report.text(true));
        assert!(!copy.exists(), "the copy goes with the line");
        assert!(
            fs::read_to_string(&locked)
                .unwrap()
                .starts_with("# 我的\r\n")
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// PIN (release read B1) — **a power loss between the record and the write leaves nothing
    /// unknown**: the record names folders made and a file not yet written, and the next removal
    /// retires them — the folders (each only if empty) and the entry's claim. A write that failed
    /// takes the record back to what it was and removes the copy it took.
    ///
    /// RED (mutations: `missing_file_kept` — `retire` returns early when the file is missing:
    /// the folders stay; `failed_write_kept` — `write_failed` leaves the attempted entry: the
    /// record claims a file Folio never created).
    #[test]
    fn a_power_loss_between_the_record_and_the_write_is_retired() {
        let root = super::super::tests::temp_dir("profile-power-loss");
        let data = root.join("data");
        let profile = root.join("a").join("b").join("profile.ps1");
        {
            let _lock = lock(&data, Asker::InApp).unwrap();
            let mut files = ProfileFiles::read(&data).unwrap();
            assert_eq!(
                files.before_write(&profile, None, std::time::UNIX_EPOCH),
                None
            );
            files.write(&data).unwrap();
        }
        // The write's own `create_dir_all` ran; the power went before the file did.
        fs::create_dir_all(profile.parent().unwrap()).unwrap();
        remove_every_line(&data);
        assert!(
            !root.join("a").exists(),
            "the folders the record names are retired"
        );
        assert!(
            ProfileFiles::read(&data).unwrap().entry(&profile).is_none(),
            "an entry with nothing left and no edition is dropped"
        );

        // A write that fails: the file is read-only, so the write refuses before it starts.
        let theirs = root.join("theirs.ps1");
        fs::write(&theirs, "# 原样\r\n").unwrap();
        let permissions = fs::metadata(&theirs).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&theirs, readonly).unwrap();
        let script = data.join(SCRIPT_DIRECTORY).join(SCRIPT_FILE_PS1);
        assert!(
            install_recorded(
                &theirs,
                &data,
                &script,
                MANAGED_LINE,
                std::time::UNIX_EPOCH,
                None,
                &ProfileRevision::read(&theirs)
            )
            .is_err()
        );
        fs::set_permissions(&theirs, permissions).unwrap();
        assert_eq!(ProfileFiles::read(&data).unwrap().entry(&theirs), None);
        assert_eq!(names(&root), ["data", "theirs.ps1"], "no copy is left");
        fs::remove_dir_all(root).unwrap();
    }

    /// PIN (release read review F3) — **a copy is deleted only while it is the copy Folio wrote.**
    /// The record names the copy and its bytes' SHA-256 before the copy is written; a power loss
    /// between the two, and a file of exactly that name appearing afterwards with other bytes,
    /// leaves a file Folio did not make — the removal keeps it, and clears the record. The copy
    /// Folio did write (same bytes) is deleted as before.
    ///
    /// RED (mutation: `copy_unchecked` — `remove_our_copy` deletes whatever stands at the
    /// recorded name: the stranger's file is gone).
    #[test]
    fn a_recorded_copy_is_deleted_only_while_it_holds_the_bytes_folio_wrote() {
        let root = super::super::tests::temp_dir("profile-copy-digest");
        let data = root.join("data");
        let profile = root.join("profile.ps1");
        let mine = "Set-Location D:\\工作\r\n";
        fs::write(&profile, mine).unwrap();
        let copy = {
            let _lock = lock(&data, Asker::InApp).unwrap();
            let mut files = ProfileFiles::read(&data).unwrap();
            let copy = files
                .before_write(&profile, None, std::time::UNIX_EPOCH)
                .expect("a file that was there gets one copy");
            files.write(&data).unwrap();
            copy
        };
        // The power went before the copy was written; a file of that name appears later.
        fs::write(&copy, "not Folio's 不是\r\n").unwrap();
        remove_every_line(&data);
        assert_eq!(
            fs::read_to_string(&copy).unwrap(),
            "not Folio's 不是\r\n",
            "a file Folio did not write stays"
        );
        assert!(ProfileFiles::read(&data).unwrap().entry(&profile).is_none());
        fs::remove_file(&copy).unwrap();

        // The copy Folio did write goes with the line.
        let wrote = enable(&profile, &data);
        let written = wrote.backup.clone().expect("one copy");
        assert_eq!(fs::read_to_string(&written).unwrap(), mine);
        undo_profile_install(edit(&wrote), &data).unwrap();
        assert!(!written.exists());
        fs::remove_dir_all(root).unwrap();
    }

    /// PIN (release read M1) — **a PowerShell that does not say where its `$PROFILE` is refuses
    /// nothing**: the line Folio recorded is removed whatever any shell answers, the silent
    /// edition is named with what is left there, and the run's exit code is 0 — so the uninstall
    /// that runs it goes on to the program. An edition the record locates is not asked at all.
    ///
    /// RED (mutations: `unlocated_refuses` — `candidates` reports a silent edition as
    /// `Fate::Refused`: exit 1; `located_asked` — `answers_for` asks every program).
    #[test]
    fn a_profile_no_shell_located_is_said_and_refuses_nothing() {
        let root = super::super::tests::temp_dir("profile-unlocated");
        let data = root.join("data");
        let profile = root.join("WindowsPowerShell").join("profile.ps1");
        enable(&profile, &data);
        let files = ProfileFiles::read(&data).unwrap();
        let asked = std::cell::RefCell::new(Vec::new());
        let answers = answers_for(
            &files,
            vec![
                PathBuf::from("C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe"),
                PathBuf::from("C:/Program Files/PowerShell/7/pwsh.exe"),
            ],
            |program| {
                asked.borrow_mut().push(program.to_path_buf());
                None
            },
        );
        assert_eq!(
            *asked.borrow(),
            [PathBuf::from("C:/Program Files/PowerShell/7/pwsh.exe")],
            "the located edition is not asked"
        );
        let report = operate_with(
            &data,
            Asker::Door,
            Action::Remove,
            Ok(MANAGED_LINE),
            |marks, files| candidates(marks, files, answers),
        );
        assert_eq!(report.exit_code(), 0, "{}", report.text(true));
        assert!(
            !profile.exists(),
            "the recorded line, and the file Folio made, are gone"
        );
        let said = report.text(false);
        assert!(
            said.contains(&format!(
                "C:/Program Files/PowerShell/7/pwsh.exe: {}",
                Text::ShellProfileUnlocated.text()
            )),
            "{said}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// One click's write against `seen`, the revision its row's check read, as
    /// [`install_for_program`] makes it after its re-probe.
    fn enable_against(
        profile: &Path,
        data: &Path,
        seen: &ProfileRevision,
    ) -> io::Result<ProfileWrite> {
        let script = data.join(SCRIPT_DIRECTORY).join(SCRIPT_FILE_PS1);
        install_recorded(
            profile,
            data,
            &script,
            MANAGED_LINE,
            std::time::UNIX_EPOCH,
            Some(PowerShellEdition::WindowsPowerShell),
            seen,
        )
    }

    /// RED (0.4.8 G7, ledger #30) — **Enable and Undo from two windows on a stale check never
    /// remove the person's own line.** Two windows' rows are drawn from one check of the profile;
    /// the first window's Enable writes the line, and the second window's Enable — made against
    /// the same, now stale, check — is refused with the reload sentence and writes nothing, so it
    /// is never answered "Added" and never offers an Undo of a line it did not write. Then the
    /// person writes a line of their own into the file, and the first window's Undo — made
    /// against the file its Enable wrote — is refused too: their line and Folio's both stay.
    ///
    /// MUTATIONS: `enable_unchecked` — drop the revision check in `install_recorded`: the second
    /// window's Enable over the line already there is answered `Ok`; `undo_unchecked` — drop the
    /// revision check in `undo_profile_install`: the Undo puts the file back to before the
    /// Enable and the person's line goes with Folio's.
    #[test]
    fn enable_and_undo_from_two_windows_on_a_stale_check_keep_the_persons_line() {
        let root = super::super::tests::temp_dir("profile-two-windows-stale");
        let data = root.join("data");
        let profile = root.join("文档").join("PowerShell").join("profile.ps1");
        fs::create_dir_all(profile.parent().unwrap()).unwrap();
        let mine = "Set-Location D:\\项目\r\n";
        fs::write(&profile, mine).unwrap();
        let check = ProfileRevision::read(&profile);
        assert!(!check.carries_the_line(), "both rows offer Enable");

        let first = enable_against(&profile, &data, &check).expect("the first window writes");
        let written = fs::read(&profile).unwrap();
        let refused = enable_against(&profile, &data, &check)
            .expect_err("the second window's check is stale");
        assert_eq!(
            refused.to_string(),
            Text::ShellProfileChangedElsewhere.text(),
            "and it says so"
        );
        assert_eq!(fs::read(&profile).unwrap(), written, "nothing was written");

        let theirs = format!("{}# 我自己的 mine\r\n", String::from_utf8(written).unwrap());
        fs::write(&profile, &theirs).unwrap();
        let undone = undo_profile_install(edit(&first), &data)
            .expect_err("the file is not what the Enable wrote");
        assert_eq!(
            undone.to_string(),
            Text::ShellProfileChangedElsewhere.text()
        );
        assert_eq!(
            fs::read_to_string(&profile).unwrap(),
            theirs,
            "the person's line and Folio's are where they were"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (0.4.8 G7 round 2, Kimi nit 1) — **a profile the check could not read is not a
    /// missing one**: its revision is neither "no file" nor bytes, and an Enable against it is
    /// refused with the reload sentence, not with whatever the writer's own read of the file
    /// says. A folder where the file should be is a path every platform refuses to read as a
    /// file with an error other than "no such file".
    ///
    /// MUTATION: `conflate` — `ProfileRevision::read` answers no file for every failed read (`.ok()`
    /// as it was): the revision is the missing file's, and the Enable meets the writer's read of
    /// a folder, which refuses with the read-only sentence instead.
    #[test]
    fn a_profile_the_check_could_not_read_is_refused_with_the_reload_sentence() {
        let root = super::super::tests::temp_dir("profile-unreadable-check");
        let data = root.join("data");
        let profile = root.join("文档").join("profile.ps1");
        fs::create_dir_all(&profile).unwrap();
        let check = ProfileRevision::read(&profile);
        assert_ne!(
            check,
            ProfileRevision::default(),
            "a folder is not a missing file"
        );
        assert!(!check.carries_the_line());
        let refused = enable_against(&profile, &data, &check).expect_err("no revision was seen");
        assert_eq!(
            refused.to_string(),
            Text::ShellProfileChangedElsewhere.text()
        );
        assert!(profile.is_dir(), "nothing was written");
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (0.4.8 G7, ledger #30) — **Undo never takes out a line the person has edited.** The
    /// click writes its line; the person edits that very line by hand; Undo is refused with the
    /// reload sentence and the edited line stays as they left it. Without the edit, the same
    /// Undo puts the file back to the bytes it held before the click, separator and all.
    ///
    /// MUTATION: `remove_by_position` — in `undo_profile_install`, drop the revision check and take
    /// out the line where the click wrote it (the file's last line): the hand-edited line is gone.
    #[test]
    fn undo_over_a_hand_edited_line_is_refused() {
        let root = super::super::tests::temp_dir("profile-undo-hand-edited");
        let data = root.join("data");
        let profile = root.join("profile.ps1");
        let mine = "# 配置 profile\r\nSet-Location D:\\工作";
        fs::write(&profile, mine).unwrap();
        let wrote = enable_against(&profile, &data, &ProfileRevision::read(&profile))
            .expect("the click writes");
        let written = fs::read_to_string(&profile).unwrap();
        let edited = written.replace(
            "# Folio shell integration v1",
            "-ErrorAction SilentlyContinue # 我改过 edited",
        );
        assert_ne!(edited, written);
        fs::write(&profile, &edited).unwrap();
        let refused = undo_profile_install(edit(&wrote), &data)
            .expect_err("the line is not the line the click wrote");
        assert_eq!(
            refused.to_string(),
            Text::ShellProfileChangedElsewhere.text()
        );
        assert_eq!(fs::read_to_string(&profile).unwrap(), edited);

        fs::write(&profile, &written).unwrap();
        undo_profile_install(edit(&wrote), &data).expect("the file is what the click wrote");
        assert_eq!(fs::read_to_string(&profile).unwrap(), mine, "byte for byte");
        fs::remove_dir_all(root).unwrap();
    }

    /// PIN — **two clicks in flight from two windows are two answers** (review C-4 of
    /// T-INTEGRATION-INJECT-4). Each answer carries the window that asked, so each window
    /// receives its own outcome and the Undo handle inside it; the second answer does not
    /// replace the first.
    ///
    /// Delivered through [`profile_installs_for`], the filter the window's event handler reads
    /// its answers through (review of round 4, item 9), and not by a filter of this test's own.
    ///
    /// RED (mutations: keep one slot, `answers.clear()` before the push in
    /// `answer_profile_install` — the first window's answer is gone; drop the window filter in
    /// `profile_installs_for` — each window receives both).
    #[test]
    fn two_windows_clicks_in_flight_each_get_their_own_answer() {
        let first = winit::window::WindowId::from(7_u64);
        let second = winit::window::WindowId::from(8_u64);
        let installed = |profile: &str| ProfileInstallOutcome::Installed {
            program: PathBuf::from("pwsh.exe"),
            edit: ProfileEdit {
                profile: PathBuf::from(profile),
                before: ProfileRevision::default(),
                after: ProfileRevision::of(Some(MANAGED_LINE.as_bytes().to_vec())),
            },
        };
        answer_profile_install(first, installed("profile A.ps1"));
        answer_profile_install(second, installed("profile B.ps1"));
        let answers = take_profile_installs();
        let mine = |window| {
            profile_installs_for(&answers, window)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(mine(first), vec![installed("profile A.ps1")]);
        assert_eq!(mine(second), vec![installed("profile B.ps1")]);
        assert!(
            take_profile_installs().is_empty(),
            "an answer is delivered once"
        );
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
            |marks, _| (marks.powershell_profiles.clone(), Report::default()),
        );
        assert_eq!(report.exit_code(), 0);
        assert_eq!(fs::read(&profile).unwrap(), original);
        assert!(report.text(false).is_empty());
        assert!(Marks::read(&root).unwrap().is_off());
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
            |_, _| (vec![profile.clone()], Report::default()),
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
            |_, _| (vec![profile.clone()], Report::default()),
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
            |_, _| (vec![profile.clone()], Report::default()),
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
                None,
                &ProfileRevision::read(&root.join(name)),
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
            None,
            &ProfileRevision::read(&root.join("7.ps1")),
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
                    std::time::UNIX_EPOCH,
                    None,
                    &ProfileRevision::read(&profile)
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
            None,
            &ProfileRevision::read(&profile),
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
            None,
            &ProfileRevision::read(&profile),
        )
        .unwrap();
        assert_eq!(fs::read(&profile).unwrap(), MANAGED_LINE.as_bytes());
    }

    /// PIN — **the two retired installer writers still serialize correctly for
    /// uninstall and historical-record fixtures.**
    ///
    /// These are the two writers that met on a clean Windows 10 machine on
    /// 2026-09-21. The product no longer reaches either installer path, but
    /// uninstall and old marks must remain safe while those historical records
    /// are supported.
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
                    None,
                    &ProfileRevision::read(&profile),
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
            |_, _| (profiles.to_vec(), Report::default()),
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
            |marks, _| {
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
            |marks, _| (marks.powershell_profiles.clone(), Report::default()),
        );
        assert!(report.files.iter().all(|f| f.fate == Fate::Unchanged));
    }
}
