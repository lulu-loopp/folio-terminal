//! **Folio's verb on the first page of the Windows 11 right-click menu** —
//! `docs/DESIGN.md` §7.4a.
//!
//! # What this is beside `context_menu`
//!
//! [`crate::context_menu`] writes two registry trees, which is the whole of what
//! a classic shell verb is. Windows 11 files every one of those under **Show
//! more options**; the page that opens first carries only `IExplorerCommand`
//! implementations declared by a **package**. So this module is not a second way
//! of doing the same thing — it is the identity the same verb needs in order to
//! be somewhere else.
//!
//! The two do not replace each other and neither switch touches the other's
//! store. The classic trees are what Windows 10 has, what a machine with no
//! package registered has, and what somebody who never turns this row on has.
//!
//! # Three facts, none of them stored
//!
//! The row's tick is read off the machine, `context_menu`'s rule for a second
//! kind of store and for its reason exactly: a boolean in `settings.json` would
//! be a copy of a truth that lives in the deployment database, free to disagree
//! with it the moment somebody removed the package from Settings ▸ Apps or
//! restored a machine from a backup.
//!
//! **But the machine is slow to ask.** `PackageManager` is a WinRT object over a
//! service; building one and running one query is tens of milliseconds, and a
//! deployment is one to three seconds. Neither may happen on the window thread —
//! so unlike `context_menu`, whose four registry opens are answered inside a
//! frame, this module has a background thread and an [`install_wake`] the way
//! the update check and the PSReadLine probe do, and a fourth state
//! ([`PackageState::Unknown`]) for the frames before the first answer lands.
//!
//! # The folder somebody moved
//!
//! A sparse package names an **external location** — the folder `folio.exe` is
//! actually in — and Folio is a program people drag from `Downloads` to
//! `C:\Tools`. After such a move the package is still registered, the item is
//! still on the first page, and clicking it finds nothing. That is worse than
//! the classic verb's version of the same failure, because it is a page Windows
//! itself curates.
//!
//! So the launch repairs it, `context_menu::reassert`'s rule carried onto a
//! heavier mechanism: a registration that points at another folder is
//! re-registered at this one, silently, because it is this user's own
//! registration being made to say what they already asked it to say. Nothing is
//! ever created that was not there — a machine with no package registered stays
//! that way through a hundred launches. The repair needs [`PACKAGE_FILE_NAME`]
//! beside the executable; where the file did not come along with the move, the
//! row says so and there is nothing to press.

use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use bt_platform::msix::{self, PACKAGE_FILE_NAME};

use crate::i18n::Text;

/// What this user's deployment database says about Folio's package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageState {
    /// Nobody has asked yet, or the answer has not come back.
    ///
    /// **A state and not a default**, for the reason `copilot_readiness` is one:
    /// the row drawn from it must not claim the package is absent, because a
    /// press on that claim would try to register a package that is already
    /// registered.
    Unknown,
    /// This Windows has no first page for a package to reach.
    Unsupported,
    /// No package of ours is registered for this user.
    Absent,
    /// Registered, pointing at this executable's own folder.
    Current { full_name: String },
    /// Registered, pointing at a folder that is not this one — the executable
    /// was moved, or a second copy registered itself.
    Elsewhere { full_name: String, at: PathBuf },
}

impl PackageState {
    /// Whether the first page carries an item right now.
    ///
    /// [`Self::Elsewhere`] counts, `ContextMenuState::Stale`'s ruling one
    /// mechanism along: there **is** an item on that page, and a row reading
    /// `Off` over it would be both untrue and a dead end, since the only control
    /// that could take it away would be denying it exists.
    #[must_use]
    pub fn registered(&self) -> bool {
        matches!(self, Self::Current { .. } | Self::Elsewhere { .. })
    }

    /// The name [`bt_platform::msix::remove`] has to be given, when there is one.
    #[must_use]
    pub fn full_name(&self) -> Option<&str> {
        match self {
            Self::Current { full_name } | Self::Elsewhere { full_name, .. } => Some(full_name),
            _ => None,
        }
    }
}

// ── what the COM server is told ─────────────────────────────────────────────

/// The folder a click means, given what Explorer handed over.
///
/// **A folder that was clicked is the folder**; a file that was clicked is its
/// folder, because the only thing this verb can do with a file is open the place
/// it is in and every other reading of "open here" would be a guess. A name with
/// nothing at it opens nothing at all: a window on a folder that is not there
/// would be a window somebody has to close.
///
/// The one impure input is a [`crate::cli::PathKind`], which is `resolve`'s own
/// discipline for the same reason — so the table can be handed in.
#[must_use]
pub fn folder_for(path: &Path, kind: crate::cli::PathKind) -> Option<PathBuf> {
    match kind {
        crate::cli::PathKind::Directory => Some(path.to_path_buf()),
        crate::cli::PathKind::File => path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf),
        crate::cli::PathKind::Absent => None,
    }
}

/// `"<exe>,0"` — the executable's own first icon, `context_menu`'s answer for
/// this question and for its reason: the picture Explorer already draws for
/// `folio.exe` is by construction the right one, and an `.ico` beside it would
/// be a second file to ship and to keep in step.
#[must_use]
pub fn verb_icon(exe: &Path) -> String {
    format!("{},0", exe.display())
}

/// Serve Explorer's class until it stops asking — the whole of
/// `--explorer-command`.
///
/// Returns the process's exit code. The language is resolved here, before the
/// first word is handed over, through the same two calls
/// `report_at_the_front_door` makes: this process has no window and no settings
/// of its own, and the menu must be in the language the user chose for
/// everything Folio says.
pub fn serve() -> i32 {
    crate::i18n::install(crate::resolved_language(
        crate::persist::SettingsStore::open().loaded().language,
    ));
    let Ok(exe) = std::env::current_exe() else {
        // Without this there is no icon to name and no program to start. There
        // is also nowhere to report it: this process has no console and no
        // window, and Explorer's answer to a class that will not start is to
        // leave the item out.
        return 1;
    };
    let verb = bt_platform::explorer_command::Verb {
        title: Text::ExplorerCommandVerb.text().to_owned(),
        icon: verb_icon(&exe),
        invoke: Box::new(move |clicked| {
            let Some(folder) = folder_for(clicked, crate::cli::machine_path_kind(clicked)) else {
                return;
            };
            // **`--cwd` and never the working directory.** A process the shell
            // starts inherits `folio.exe`'s own folder as its current directory
            // (`cli.rs`'s header, measured by the spike), so the place has to
            // travel as an argument. This is the same launch the classic verb's
            // `command` value spells.
            //
            // Through `quiet_command` because that is the one door (§7.40 ①):
            // this process has no console, and a child Windows had to find a
            // console for is a Windows Terminal window opening in front of the
            // window somebody actually asked for.
            let _ = bt_platform::quiet_command(&exe)
                .arg("--cwd")
                .arg(folder)
                .spawn();
        }),
    };
    match bt_platform::explorer_command::serve(verb) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("BT_EXPLORER_COMMAND {error}");
            1
        }
    }
}

// ── the machine's answer, and the thread that fetches it ────────────────────

/// The last answer any thread got, and what the row is drawn from.
static KNOWN: Mutex<Option<PackageState>> = Mutex::new(None);

/// Whether a deployment call is in flight.
///
/// One at a time, and the door is shut rather than queued: two registrations of
/// the same package racing is not a thing the deployment API is owed, and a user
/// who presses a switch twice means the second press.
static BUSY: AtomicBool = AtomicBool::new(false);

/// The outcome of the last press, waiting for the frame that will show it.
static OUTCOME: Mutex<Option<Result<bool, String>>> = Mutex::new(None);

/// How a finished job asks for a frame — [`install_wake`].
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Install the wake, once, at startup.
///
/// The same shape as `update::install_wake` and for the same reason: the answer
/// arrives on a thread that has no idea which window is up, and the row it
/// changes is usually behind a modal that nothing else is going to repaint.
pub fn install_wake<F: Fn() + Send + Sync + 'static>(wake: F) {
    let _ = WAKE.set(Box::new(wake));
}

fn wake() {
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

/// Whether this Windows has a first page at all.
///
/// Asked of the kernel once and remembered: it cannot change while this process
/// runs, and it gates whether a row is drawn at all — which is a question every
/// frame of the settings dialog asks.
#[must_use]
pub fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    #[cfg(windows)]
    let answer = || msix::supports_primary_context_menu(msix::windows_build());
    #[cfg(not(windows))]
    let answer = || false;
    *SUPPORTED.get_or_init(answer)
}

/// `folio.msix`, beside the executable, when it is there.
///
/// The package file ships **in the archive** rather than being downloaded or
/// generated, so "is it beside the exe" is the same question as "did the whole
/// archive get extracted". A machine where it is missing can still have the
/// classic verb and cannot have this one, and the row says which.
#[must_use]
pub fn package_file() -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()?
        .parent()?
        .join(PACKAGE_FILE_NAME);
    beside.is_file().then_some(beside)
}

/// What the row is drawn from.
#[must_use]
pub fn state() -> PackageState {
    KNOWN
        .lock()
        .expect("the package state is not held across a panic")
        .clone()
        .unwrap_or(PackageState::Unknown)
}

fn remember(state: PackageState) {
    *KNOWN
        .lock()
        .expect("the package state is not held across a panic") = Some(state);
}

/// Read the deployment database, on the thread that calls this.
///
/// Not public: every caller of it is on a thread of its own, and a version of
/// this that could be called from the window thread is a version somebody calls
/// from the window thread.
#[cfg(windows)]
fn read_state() -> PackageState {
    if !supported() {
        return PackageState::Unsupported;
    }
    let registered = match msix::registered() {
        Ok(Some(registered)) => registered,
        // A failure to *ask* is reported as absence rather than as an error the
        // row could show. What the row would say is "Windows would not answer",
        // which is neither actionable nor a state a press could change; the next
        // launch asks again.
        Ok(None) | Err(_) => return PackageState::Absent,
    };
    let here = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    match (registered.external_path, here) {
        (Some(at), Some(here)) if !same_folder(&at, &here) => PackageState::Elsewhere {
            full_name: registered.full_name,
            at,
        },
        _ => PackageState::Current {
            full_name: registered.full_name,
        },
    }
}

#[cfg(not(windows))]
fn read_state() -> PackageState {
    PackageState::Unsupported
}

/// Whether two paths name the same folder.
///
/// **Compared after canonicalisation where the operating system will do it**, and
/// case-insensitively as a fallback, because the two strings come from different
/// places: one is the deployment database's record of what was registered and one
/// is `current_exe`'s answer, and they differ over a trailing separator, over
/// `C:` against `c:`, and over an 8.3 short name in a path somebody registered
/// from a console. Reading any of those as "the folder moved" would re-register
/// the package on every launch.
#[must_use]
pub fn same_folder(left: &Path, right: &Path) -> bool {
    let canonical = |path: &Path| std::fs::canonicalize(path).ok();
    if let (Some(left), Some(right)) = (canonical(left), canonical(right)) {
        return left == right;
    }
    let trim = |path: &Path| {
        path.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    trim(left) == trim(right)
}

/// Ask the machine, and repair a registration that names another folder.
///
/// Started at launch, on a thread of its own. Nothing on the path to the first
/// frame waits for it.
pub fn begin_probe() {
    if !supported() {
        remember(PackageState::Unsupported);
        return;
    }
    std::thread::spawn(|| {
        let state = read_state();
        // The repair, and the only place this module writes without being
        // pressed. `Absent` is left alone on purpose — see the module header.
        if let PackageState::Elsewhere { .. } = &state
            && let Some(package) = package_file()
            && let Some(here) = package.parent()
        {
            match msix::register(&package, here) {
                Ok(()) => {
                    remember(read_state());
                    wake();
                    return;
                }
                Err(error) => {
                    // Silent to the user, on `reassert`'s footing: the entry that
                    // is there goes on being whatever it was, the next launch
                    // tries again, and there is no window yet to put a card on.
                    eprintln!("BT_EXPLORER_PACKAGE repair refused — {error}");
                }
            }
        }
        remember(state);
        wake();
    });
}

/// Register the package, or take it back off — the row's whole action.
///
/// Returns whether a job was started. `false` is a press with nothing to do:
/// another job is running, or the file this one would need is not there.
pub fn request(install: bool) -> bool {
    if BUSY.swap(true, Ordering::AcqRel) {
        return false;
    }
    let package = package_file();
    if install && package.is_none() {
        BUSY.store(false, Ordering::Release);
        report(Err(Text::ExplorerFirstPageNoPackage.text().to_owned()));
        return true;
    }
    std::thread::spawn(move || {
        let outcome = if install {
            let package = package.expect("an install with no package file was refused above");
            match package.parent() {
                Some(here) => msix::register(&package, here).map(|()| true),
                None => Err(Text::ExplorerFirstPageNoPackage.text().to_owned()),
            }
        } else {
            // **The name is fetched here and not carried in from the press.** The
            // cached answer can be [`PackageState::Unknown`] — the first probe of
            // a launch has not landed — and a removal that read `Unknown` as "no
            // package" would report success over a package that is still
            // registered. This thread can afford the question; the one that took
            // the press could not.
            match read_state().full_name() {
                Some(full_name) => msix::remove(full_name).map(|()| false),
                // Nothing registered and a press asking for that: the machine is
                // already where the press wanted it.
                None => Ok(false),
            }
        };
        remember(read_state());
        report(outcome);
        BUSY.store(false, Ordering::Release);
    });
    true
}

fn report(outcome: Result<bool, String>) {
    *OUTCOME
        .lock()
        .expect("the package outcome is not held across a panic") = Some(outcome);
    wake();
}

/// The outcome of a press, taken once by the window that shows it.
pub fn take_outcome() -> Option<Result<bool, String>> {
    OUTCOME
        .lock()
        .expect("the package outcome is not held across a panic")
        .take()
}

/// **The row's own sentence** — a fact about this machine, not about the switch.
///
/// A function rather than a table entry, on `context_menu::row_description`'s
/// footing: which of the three sentences is true depends on the world, and the
/// module that owns the world is the one that should be asked.
#[must_use]
pub fn row_description() -> &'static str {
    if package_file().is_none() {
        return Text::DescExplorerFirstPageNoPackage.text();
    }
    match state() {
        PackageState::Elsewhere { .. } => Text::DescExplorerFirstPageElsewhere.text(),
        _ => Text::DescExplorerFirstPage.text(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::PathKind;

    /// PIN — **a folder opens itself and a file opens its folder.**
    ///
    /// The whole of what `Invoke` decides, and the two halves are not the same
    /// rule: Explorer hands this verb a folder for `Directory` and for
    /// `Directory\Background`, and a *file* only ever arrives from a selection
    /// somebody made across item types. A name with nothing at it must open
    /// nothing — a window on a folder that is not there is a window to close.
    ///
    /// MUTATION: answer `Some(path)` for `Absent` and the last assertion goes
    /// red, which is a click on a deleted folder opening a shell in nowhere.
    #[test]
    fn a_click_names_a_folder_a_file_names_its_folder_and_nothing_names_nothing() {
        let folder = Path::new(r"D:\Developer\folio");
        assert_eq!(
            folder_for(folder, PathKind::Directory),
            Some(folder.to_path_buf())
        );
        assert_eq!(
            folder_for(&folder.join("Cargo.toml"), PathKind::File),
            Some(folder.to_path_buf())
        );
        assert_eq!(folder_for(folder, PathKind::Absent), None);
        // A file with no parent is a name and not a place.
        assert_eq!(folder_for(Path::new("Cargo.toml"), PathKind::File), None);
    }

    /// PIN — **a moved registration is still an item on the first page.**
    ///
    /// The switch is drawn from [`PackageState::registered`], and `Elsewhere`
    /// counts: `ContextMenuState::Stale`'s ruling one mechanism along. A row
    /// reading `Off` over an entry that is genuinely on that page would be untrue
    /// and would also leave the reader no control that could take it away.
    ///
    /// `Unknown` is the other half and it is not absence — a press made on a row
    /// that had read it as absence would try to register a package that may
    /// already be registered.
    ///
    /// MUTATION: make `registered` answer only for `Current` and the third
    /// assertion goes red, which is a moved folder leaving an item on the first
    /// page that the switch says is not there.
    #[test]
    fn what_the_switch_reads_off_the_deployment_database() {
        let elsewhere = PackageState::Elsewhere {
            full_name: "WeiyiShi.Folio_0.1.1.0_x64__abc".to_owned(),
            at: PathBuf::from(r"C:\Tools\folio"),
        };
        let current = PackageState::Current {
            full_name: "WeiyiShi.Folio_0.1.1.0_x64__abc".to_owned(),
        };
        assert!(!PackageState::Absent.registered());
        assert!(current.registered());
        assert!(elsewhere.registered());
        assert!(!PackageState::Unknown.registered());
        assert!(!PackageState::Unsupported.registered());
        assert_eq!(elsewhere.full_name(), current.full_name());
        assert_eq!(PackageState::Unknown.full_name(), None);
        assert_eq!(PackageState::Absent.full_name(), None);
    }

    /// PIN — **a trailing separator and a drive letter's case are not a move.**
    ///
    /// The two strings compared here come from different places — the deployment
    /// database's record and `current_exe` — and reading either difference as
    /// "the folder moved" would re-register the package on every single launch,
    /// which is a three-second deployment somebody pays for at every start.
    ///
    /// MUTATION: compare the two paths with `==` and the first two assertions go
    /// red.
    #[test]
    fn the_same_folder_spelled_two_ways_is_one_folder() {
        assert!(same_folder(
            Path::new(r"C:\Tools\folio"),
            Path::new(r"c:\tools\folio\")
        ));
        assert!(same_folder(
            Path::new(r"D:\Developer\folio\"),
            Path::new(r"D:\Developer\folio")
        ));
        assert!(!same_folder(
            Path::new(r"C:\Tools\folio"),
            Path::new(r"C:\Tools\folio2")
        ));
    }

    /// PIN — **the icon is the executable's own first, and the words are the
    /// first page's.**
    ///
    /// The classic verb says `Open Folio here`, among third-party verbs that all
    /// end that way; this one sits between `Copy` and `Rename`, where Windows'
    /// own form is `Open in …`. Two labels for one action is deliberate and is
    /// argued in DESIGN §7.4a — what a test can hold is that neither is empty
    /// and that this one is the one the manifest advertises.
    #[test]
    fn the_first_page_item_wears_this_binary_and_says_open_in_folio() {
        let exe = Path::new(r"C:\Tools\folio\folio.exe");
        assert_eq!(verb_icon(exe), r"C:\Tools\folio\folio.exe,0");
        assert_eq!(
            Text::ExplorerCommandVerb.in_lang(crate::i18n::Lang::English),
            "Open in Folio"
        );
        assert_eq!(
            Text::ExplorerCommandVerb.in_lang(crate::i18n::Lang::Chinese),
            "在 Folio 中打开"
        );
    }
}
