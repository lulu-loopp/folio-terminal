//! Where `session.json` and `settings.json` live, when they are written, and
//! what a leftover sentinel means.
//!
//! `bt-persist` is deliberately timer-free and path-free: it knows how to read
//! and write two JSON documents given explicit paths, and how to degrade when
//! either is missing or damaged. Everything it left to "the chrome slice that
//! consumes this crate" is here — the actual storage directory, the debounce
//! duration, when `mark_dirty` fires, and who calls `probe_sentinel` at
//! startup.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use bt_persist::{
    Debouncer, ExitState, ReadReport, SessionV1, SettingsV1, WriteAlertAction, WriteFailureTracker,
    create_sentinel, probe_sentinel, read_session, read_settings, remove_sentinel,
    write_session_atomic, write_settings_atomic,
};

/// docs/M2-persistence-schema-v1.md §5.1 rules "debounce roughly 1-2 seconds
/// after a meaningful change", and leaves the exact figure to the call site.
/// The slower end of the band: a session write is never urgent, and every
/// hundred milliseconds spent waiting is a divider drag that does not turn into
/// its own disk write.
const SESSION_DEBOUNCE: Duration = Duration::from_millis(1_500);

/// The session file, its sentinel, and the debounce that stands between a
/// change and the disk.
pub struct SessionStore {
    session_path: PathBuf,
    sentinel_path: PathBuf,
    session: SessionV1,
    debouncer: Debouncer,
    failures: WriteFailureTracker,
    /// True once the sentinel for *this* run exists, so a clean exit knows
    /// there is something to remove.
    armed: bool,
}

impl SessionStore {
    /// Read the previous session and arm this one's sentinel.
    ///
    /// Every failure path here is non-fatal by construction: `read_session`
    /// never returns an error (a failure to load *is* the default document),
    /// and a storage directory that cannot be created leaves a store that
    /// simply never writes. A terminal that refuses to start because it could
    /// not write a layout file would be a worse product than one that forgets
    /// its layout.
    pub fn open() -> Self {
        let dir = storage_dir();
        let session_path = dir.join("session.json");
        let sentinel_path = dir.join("session.lock");
        let writable = std::fs::create_dir_all(&dir).is_ok();
        // Probe *before* creating: creating first would make every probe after
        // the first report a crash.
        let previous_exit = probe_sentinel(&sentinel_path).unwrap_or(ExitState::Normal);
        let (session, report, degradation) = read_session(&session_path);
        // §5.4 case 1 — no file yet — is the normal first run and must not
        // alert; every other non-`Loaded` outcome must (§5.3: "explicit alert,
        // never pretend it succeeded").
        if let ReadReport::FellBackToDefaults { reason } = &report {
            eprintln!("BT_PERSIST session.json fell back to defaults: {reason:?}");
        }
        if !degradation.is_clean() {
            eprintln!(
                "BT_PERSIST session.json degraded: {} clamped ratios, {} unknown leaves",
                degradation.clamped_ratios, degradation.unknown_leaves
            );
        }
        if previous_exit == ExitState::Crashed {
            eprintln!("BT_PERSIST previous session did not reach its clean-exit path");
        }
        let armed = writable && create_sentinel(&sentinel_path).is_ok();
        Self {
            session_path,
            sentinel_path,
            session,
            debouncer: Debouncer::new(),
            failures: WriteFailureTracker::new(),
            armed,
        }
    }

    /// The session document as it was read. The caller owns what the fields
    /// mean; this type owns only when they reach the disk.
    pub fn loaded(&self) -> &SessionV1 {
        &self.session
    }

    /// Replace the in-memory document and start the debounce window. Called on
    /// a meaningful change: a tree edit, or the end of a resize.
    pub fn record(&mut self, session: SessionV1, now: Instant) {
        if self.session == session {
            return;
        }
        self.session = session;
        self.debouncer.mark_dirty(now);
    }

    /// When the event loop should wake to write, if it should.
    pub fn deadline(&self) -> Option<Instant> {
        self.debouncer
            .is_dirty()
            .then(|| Instant::now() + SESSION_DEBOUNCE)
    }

    /// Write if the quiet window has elapsed.
    pub fn flush_if_due(&mut self, now: Instant) {
        if self.debouncer.should_flush(now, SESSION_DEBOUNCE) {
            self.flush();
        }
    }

    /// Write now, whatever the debounce says. The clean-exit path uses this:
    /// a pending change must not be lost because the window closed 300ms after
    /// it happened.
    pub fn flush(&mut self) {
        if !self.debouncer.is_dirty() {
            return;
        }
        let result = write_session_atomic(&self.session_path, &self.session);
        if self.failures.record(result.is_ok()) == WriteAlertAction::AlertOnce
            && let Err(error) = &result
        {
            // §5.3: one alert per failure streak, not one per attempt. A full
            // disk must not turn into a message every 1.5 seconds.
            eprintln!("BT_PERSIST could not write session.json: {error}");
        }
        if result.is_ok() {
            self.debouncer.mark_flushed();
        }
    }

    /// Flush anything pending and drop this run's sentinel. Idempotent.
    pub fn close(&mut self) {
        self.flush();
        if self.armed {
            let _ = remove_sentinel(&self.sentinel_path);
            self.armed = false;
        }
    }
}

/// `settings.json` and when it reaches the disk.
///
/// No debouncer, unlike [`SessionStore`], and §1.1 is explicit about why the two
/// files are separate at all: "设置改动应立即落盘(用户在设置面板点一下就该生效并
/// 持久,丢失更痛)". A settings write happens when a human clicks a row in a
/// dialog — it cannot arrive at the rate divider drags do, so there is nothing
/// to coalesce and a quiet window would only be a window in which the choice can
/// be lost.
pub struct SettingsStore {
    path: PathBuf,
    settings: SettingsV1,
    failures: WriteFailureTracker,
}

impl SettingsStore {
    /// Read `settings.json`, falling back to defaults on every failure — same
    /// contract as [`SessionStore::open`], and for the same reason: a terminal
    /// that refuses to start because it could not read a preferences file would
    /// be a worse product than one that starts with the default preferences.
    pub fn open() -> Self {
        let dir = storage_dir();
        let path = dir.join("settings.json");
        let _ = std::fs::create_dir_all(&dir);
        let (settings, report) = read_settings(&path);
        // §5.4 case 1 — no file yet — is the normal first run and must not alert.
        if let ReadReport::FellBackToDefaults { reason } = &report {
            eprintln!("BT_PERSIST settings.json fell back to defaults: {reason:?}");
        }
        Self {
            path,
            settings,
            failures: WriteFailureTracker::new(),
        }
    }

    /// The settings as they currently stand.
    pub fn loaded(&self) -> &SettingsV1 {
        &self.settings
    }

    /// Record a change and put it on disk now. Returns whether anything changed,
    /// so a caller can skip the repaint when a user picks the value already set.
    pub fn store(&mut self, settings: SettingsV1) -> bool {
        if self.settings == settings {
            return false;
        }
        self.settings = settings;
        let result = write_settings_atomic(&self.path, &self.settings);
        if self.failures.record(result.is_ok()) == WriteAlertAction::AlertOnce
            && let Err(error) = &result
        {
            // §5.3: one alert per failure streak, not one per attempt.
            eprintln!("BT_PERSIST could not write settings.json: {error}");
        }
        true
    }
}

/// The directory this build wrote its files under before the product was named,
/// and the only reason this module knows the old brand at all.
///
/// It exists for one startup, on one machine, once: see [`relocate`].
const PREVIOUS_STORAGE_NAME: &str = "BetterTerminal";

/// The directory the product writes under, which is its name.
const STORAGE_NAME: &str = "Folio";

/// `%APPDATA%\Folio\` (§1.2). Falls back to the process temp directory when the
/// environment has no `APPDATA` — the same reasoning as the panic log's: a
/// diagnostic that cannot be written is worse than one written somewhere less
/// convenient.
///
/// **Resolved once per process, and the move a rename owes the user happens on
/// that first call.** Three callers ask for this directory — the session store,
/// the settings store and the bash script's install path — and whichever asks
/// first is the one that pays for the relocation; the other two find it done.
/// The alternative, a relocation stapled to `main`, would leave the answer
/// depending on whether that line ran, which is exactly the kind of ordering a
/// `OnceLock` exists to remove.
pub fn storage_dir() -> PathBuf {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY
        .get_or_init(|| {
            let root = match std::env::var_os("APPDATA") {
                Some(appdata) => PathBuf::from(appdata),
                None => std::env::temp_dir(),
            };
            let current = root.join(STORAGE_NAME);
            let previous = root.join(PREVIOUS_STORAGE_NAME);
            match relocate(&previous, &current) {
                Relocation::Nothing | Relocation::AlreadyHere => current,
                Relocation::Moved => {
                    eprintln!(
                        "BT_PERSIST moved {} to {}",
                        previous.display(),
                        current.display()
                    );
                    current
                }
                Relocation::Failed(error) => {
                    // Fail-soft, and deliberately the *old* directory rather than
                    // an empty new one: a user whose files could not be moved
                    // keeps reading and writing the files they already have.
                    // Starting fresh beside them would present as "the terminal
                    // forgot everything" while the data sat one directory away.
                    eprintln!(
                        "BT_PERSIST could not move {} to {}: {error} — continuing in the old \
                         directory",
                        previous.display(),
                        current.display()
                    );
                    previous
                }
            }
        })
        .clone()
}

/// What [`relocate`] found, and did.
///
/// No `PartialEq`: the failure arm carries the `io::Error` the user has to be
/// told about, and two of those are not comparable in any sense this code means.
#[derive(Debug)]
enum Relocation {
    /// No directory under the old name: a first run, or a machine that only ever
    /// knew this one. Nothing to carry.
    Nothing,
    /// Both names exist. The move already happened — or the user made the new
    /// directory themselves — and either way the current one is authoritative.
    /// The old one is left exactly where it is: it is not this code's to delete,
    /// and a stale copy is cheaper than a wrong deletion.
    AlreadyHere,
    /// The whole directory arrived under the new name.
    Moved,
    /// It could not, and the old directory is still the one with the files in it.
    Failed(std::io::Error),
}

/// Carry `previous` over to `current`, once, if that is what the disk says is
/// needed.
///
/// **One rename of the directory itself, not a walk that copies files.** The
/// contents are `session.json`, `settings.json`, a lock sentinel and the
/// installed shell-integration script — a set this code should not have to
/// enumerate, and would be wrong about the moment anything is added. A directory
/// rename within one volume is also atomic where a copy is not: it either
/// happened or it did not, and there is no state in which half the user's
/// settings are under each name.
///
/// The three states are decided by the two directories alone, so the answer does
/// not depend on a flag file that could be lost, and calling this again after a
/// successful move returns [`Relocation::Nothing`] rather than moving anything a
/// second time.
fn relocate(previous: &Path, current: &Path) -> Relocation {
    if !previous.is_dir() {
        return Relocation::Nothing;
    }
    if current.exists() {
        return Relocation::AlreadyHere;
    }
    // The parent is `%APPDATA%` and already exists — `previous` is inside it —
    // so there is nothing to create before the rename.
    match std::fs::rename(previous, current) {
        Ok(()) => Relocation::Moved,
        Err(error) => Relocation::Failed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private `%APPDATA%` for one test, with nothing of the real one in it.
    ///
    /// Named from the test rather than from a counter so a failure leaves a
    /// directory whose name says which case left it, and cleaned on the way *in*
    /// as well as out: a run that panicked half way through must not hand the
    /// next run a directory that already has both names in it.
    fn appdata(case: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("bt-app-relocate-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private APPDATA for this test");
        root
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a file has a parent")).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    /// PIN — state one of three: nothing was ever written under the old name, so
    /// there is nothing to carry and nothing is invented.
    ///
    /// This is every machine that meets this product for the first time, and the
    /// claim worth making about it is the negative one: the relocation does not
    /// create the directory. Creating it here would take the decision away from
    /// `SessionStore::open`, which is the code that knows whether a directory it
    /// cannot create is fatal (it is not).
    ///
    /// Red gate: drop the `previous.is_dir()` guard and a first run starts
    /// reporting a move it did not make.
    #[test]
    fn a_machine_that_never_knew_the_old_name_has_nothing_to_carry() {
        let root = appdata("fresh");
        let previous = root.join(PREVIOUS_STORAGE_NAME);
        let current = root.join(STORAGE_NAME);
        assert!(
            matches!(relocate(&previous, &current), Relocation::Nothing),
            "neither name exists, so there is nothing to carry"
        );
        assert!(
            !current.exists(),
            "the relocation does not create a directory"
        );
        assert!(!previous.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — state two of three: the whole directory arrives under the new name,
    /// with everything that was in it.
    ///
    /// The nested file is the point rather than decoration. The directory holds
    /// `session.json`, `settings.json`, a lock sentinel *and* an installed
    /// `shell-integration/folio.bash`, and a relocation written as a list of
    /// known filenames would carry the two documents and silently leave the
    /// script — which would then be rewritten on next use and look fine, hiding
    /// the fact that the same bug drops whatever is added next.
    ///
    /// Red gate: replace the directory rename with a copy of the two JSON
    /// documents and the nested assertion fails.
    #[test]
    fn the_whole_old_directory_arrives_under_the_new_name() {
        let root = appdata("carry");
        let previous = root.join(PREVIOUS_STORAGE_NAME);
        let current = root.join(STORAGE_NAME);
        write(&previous.join("settings.json"), r#"{"version":4}"#);
        write(&previous.join("session.json"), r#"{"version":6}"#);
        write(
            &previous.join("shell-integration/folio.bash"),
            "# installed",
        );

        assert!(
            matches!(relocate(&previous, &current), Relocation::Moved),
            "the old name exists and the new one does not: this is the one start \
             that moves anything"
        );

        assert_eq!(
            read(&current.join("settings.json")).as_deref(),
            Some(r#"{"version":4}"#)
        );
        assert_eq!(
            read(&current.join("session.json")).as_deref(),
            Some(r#"{"version":6}"#)
        );
        assert_eq!(
            read(&current.join("shell-integration/folio.bash")).as_deref(),
            Some("# installed"),
            "a rename carries what it does not have to know the name of"
        );
        assert!(
            !previous.exists(),
            "the old name is gone, so the next start takes the first branch"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — state three of three: a directory that already moved is not moved
    /// again, and the old name — if something recreated it — cannot overwrite it.
    ///
    /// The second start is the ordinary case for every upgraded machine, and the
    /// dangerous one: a relocation that ran unconditionally would, on a machine
    /// where anything ever recreated the old directory, replace a live settings
    /// file with a stale one. So the assertion is not merely `AlreadyHere` — it
    /// is that the current document is *still the current document*.
    ///
    /// Red gate: drop the `current.exists()` guard. `fs::rename` onto an existing
    /// directory fails on Windows, so the visible symptom would be a spurious
    /// failure banner on every start; on a platform where it succeeds it is data
    /// loss. The guard is what makes the operation idempotent rather than lucky.
    #[test]
    fn a_directory_that_already_moved_is_left_alone() {
        let root = appdata("already");
        let previous = root.join(PREVIOUS_STORAGE_NAME);
        let current = root.join(STORAGE_NAME);
        write(&previous.join("settings.json"), "stale");
        write(&current.join("settings.json"), "live");

        assert!(
            matches!(relocate(&previous, &current), Relocation::AlreadyHere),
            "both names exist, so the move has already happened"
        );

        assert_eq!(
            read(&current.join("settings.json")).as_deref(),
            Some("live"),
            "the directory in use outranks the one it replaced"
        );
        assert_eq!(
            read(&previous.join("settings.json")).as_deref(),
            Some("stale"),
            "and the old one is left where it is rather than deleted"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — a move that cannot happen loses nothing.
    ///
    /// The failure is made by pointing the new name at a directory whose parent
    /// does not exist, which is what an `%APPDATA%` that has gone away underneath
    /// a running process looks like; the realistic cause on a live machine is a
    /// second process holding the old directory open. Either way the outcome the
    /// user must get is the same one: their files are still there, under the name
    /// they were under, and [`storage_dir`] keeps reading them.
    ///
    /// Red gate: turn the error arm into `Relocation::Moved` — or make
    /// `storage_dir` return `current` regardless — and an upgrade that could not
    /// move the directory presents as a terminal that forgot every setting, with
    /// the settings sitting one directory away.
    #[test]
    fn a_move_that_cannot_happen_leaves_the_files_where_they_are() {
        let root = appdata("refused");
        let previous = root.join(PREVIOUS_STORAGE_NAME);
        let current = root.join("no-such-parent").join(STORAGE_NAME);
        write(&previous.join("settings.json"), "mine");

        assert!(
            matches!(relocate(&previous, &current), Relocation::Failed(_)),
            "a rename into a directory that does not exist cannot succeed"
        );

        assert_eq!(
            read(&previous.join("settings.json")).as_deref(),
            Some("mine"),
            "the files are exactly where they were"
        );
        assert!(!current.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — the two names, spelled out.
    ///
    /// The old one is a fact about disks that already exist and can never be
    /// edited to something else; the new one is the product's name and is what
    /// `%APPDATA%\Folio` in `docs/M2-persistence-schema-v1.md` §1.2 means.
    #[test]
    fn the_storage_directory_is_named_for_the_product() {
        assert_eq!(STORAGE_NAME, crate::APP_NAME);
        assert_eq!(PREVIOUS_STORAGE_NAME, "BetterTerminal");
    }
}
