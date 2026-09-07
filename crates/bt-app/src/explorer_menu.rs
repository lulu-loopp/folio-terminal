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
//! The two do not replace each other and neither registration touches the
//! other's store. The classic trees are what Windows 10 has, what a machine with
//! no package registered has, and what somebody who never asks for the first
//! page has.
//!
//! # One row, one switch (user ruling 2026-09-07)
//!
//! They were two switches that morning and a picker of three places by the
//! afternoon; the ruling that closed the day cut the picker too. The reader's
//! question is not *where* — it is **whether Folio is in Explorer's menu**, and
//! a machine that can put the verb in two places should put it in both, because
//! a reader who wanted it in only one of them is a reader nobody has met.
//!
//! So the row is a switch. **On is everything this machine can do**
//! ([`place_when_on`]): Windows 11 with `folio.msix` in this folder means the
//! package **and** the classic entry; anywhere else it means the classic entry
//! alone. **Off is neither.** The middle place is still a place —
//! [`ExplorerPlace::ShowMoreOptions`] is what On means on most machines — but it
//! is no longer something to choose on a machine that can do better.
//!
//! The first-run card's one switch has always meant exactly this, and now says
//! so through the same function: [`crate::first_run::ExplorerShape::place`]
//! calls [`place_when_on`]. Two surfaces asking one question cannot drift when
//! one function answers it.
//!
//! Nothing about the storage changed: the classic half is read off the registry,
//! the package half off the deployment database, and `settings.json` holds no
//! copy of either. The switch is **On when the verb is anywhere in that menu**,
//! which is [`place`] read against `Off`.
//!
//! Because On means different things on different machines, the row's own line
//! says which ([`description_for`]) — the greyed answer that used to carry that
//! reason went with the picker, and the sentence stayed. An answer nobody can
//! choose needs no card explaining that it was refused either, so the toast that
//! used to say `folio.msix is not beside folio.exe` has no press left to fire
//! on.
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

// ── the one switch's two states, and the places they mean ───────────────────

/// Where Folio's verb stands in Explorer's right-click menu (user ruling
/// 2026-09-07).
///
/// **Not a set of answers a reader picks from** since the switch replaced the
/// picker: it is what the switch's two states *mean* on the machine in front of
/// them. `Off` is the switch off; the two under it are the two shapes On can
/// take, and which one it takes is [`place_when_on`]'s answer rather than
/// anybody's choice.
///
/// Ordered as a ladder all the same — each is the one above it plus a
/// registration — because that is the order [`place`] reads them back in.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExplorerPlace {
    /// Neither registration. The factory state, and what the switch off leaves.
    #[default]
    Off,
    /// The classic entry alone — the two registry trees [`crate::context_menu`]
    /// writes. This is the whole menu on Windows 10 and the **Show more
    /// options** page on Windows 11, and it is what On means on every machine
    /// that has no `folio.msix` to register.
    ShowMoreOptions,
    /// The package registered **and** the classic entry kept.
    ///
    /// Both, and not the package alone: Windows 10 has no first page, a machine
    /// that later loses the package still has the verb, and the two pages do not
    /// know about each other (DESIGN §7.4a). It is the pair the first-run card
    /// has always spent together, and since the switch it is what On means
    /// wherever this machine can honour it.
    FirstPage,
}

impl ExplorerPlace {
    /// Whether this place wants the classic registry trees.
    #[must_use]
    pub fn classic(self) -> bool {
        matches!(self, Self::ShowMoreOptions | Self::FirstPage)
    }

    /// Whether this place wants the package registered.
    #[must_use]
    pub fn package(self) -> bool {
        matches!(self, Self::FirstPage)
    }

    /// Whether the verb is in that menu at all — the switch's own state.
    ///
    /// One reading and not two, so that what a row draws and what a press means
    /// cannot come apart: the switch is on exactly when this place is not
    /// [`Self::Off`].
    #[must_use]
    pub fn on(self) -> bool {
        self != Self::Off
    }
}

/// **The highest place this machine can honour** — what On means (user ruling
/// 2026-09-07).
///
/// The one function behind both switches. The first-run card has always spent
/// the pair together where it could, and the Settings row now says the same
/// thing; sharing the answer is what makes "the card and the row mean the same"
/// a fact about the code rather than a promise in a comment.
///
/// Never [`ExplorerPlace::Off`]: On is a request for something, and the machine
/// fact handed in only decides how much of it there is to give.
#[must_use]
pub fn place_when_on(first_page_offered: bool) -> ExplorerPlace {
    if first_page_offered {
        ExplorerPlace::FirstPage
    } else {
        ExplorerPlace::ShowMoreOptions
    }
}

/// The card a press on this row owes when it lands.
///
/// **Named for where the verb ends up, never for what a registration did.** A
/// press moves the verb to a place, so the reader is owed the place; `Taken off
/// the first page` was a fourth sentence saying half of what `Under Show more
/// options` says whole, and it retired with the second row (user ruling
/// 2026-09-07).
///
/// Three cards for a switch of two states is not a contradiction: On lands in
/// one of two places, and a card that named the switch rather than the place
/// would tell a reader on Windows 10 that the verb is on a page their Windows
/// does not have.
///
/// **The first page has two cards and the difference is who registered it**
/// (§7.4b). Where the registration was made by this process the card says so
/// and says that Explorer may not have caught up yet, because that is the case
/// where it may not have; where the package was already registered before this
/// window opened — the press that deploys nothing — the plain card is the true
/// one. `refresh_pending` is [`shell_refresh_pending`], handed in on this
/// module's rule for every fact about the world.
#[must_use]
pub fn place_toast(place: ExplorerPlace, refresh_pending: bool) -> Text {
    match place {
        ExplorerPlace::Off => Text::ContextMenuRemovedToast,
        ExplorerPlace::ShowMoreOptions => Text::ContextMenuAddedToast,
        ExplorerPlace::FirstPage if refresh_pending => Text::ExplorerFirstPageAddedRestartToast,
        ExplorerPlace::FirstPage => Text::ExplorerFirstPageAddedToast,
    }
}

/// What the machine says the verb's place is right now.
///
/// **The registration outranks the classic entry**, which is the same reading
/// [`PackageState::registered`] already makes one level down: a package that is
/// registered puts an item on the first page, and a row reading anything else
/// over it would name a place the verb is not. A machine holding the package and
/// no classic tree is therefore `FirstPage` too — it is the state a build with
/// two switches could be left in, and the honest name for it is the highest
/// place the verb actually occupies. The switch over it reads `On` there, and
/// the next `Off` takes both away.
///
/// What the switch shows is [`ExplorerPlace::on`] of this answer; the place
/// itself is what the row's line is written from and what a press carries.
#[must_use]
pub fn place(classic: bool, package_registered: bool) -> ExplorerPlace {
    if package_registered {
        ExplorerPlace::FirstPage
    } else if classic {
        ExplorerPlace::ShowMoreOptions
    } else {
        ExplorerPlace::Off
    }
}

/// Whether [`ExplorerPlace::FirstPage`] is a place this machine can honour —
/// what [`place_when_on`] is asking about.
///
/// Both halves, because either one missing makes the same item undrawable: a
/// Windows with no first page to reach, or a folder with no `folio.msix` to
/// register. A pure function of the two facts so that both machines can be
/// pinned by a test rather than by whichever one it runs on.
#[must_use]
pub fn first_page_offered(supported: bool, package_beside_exe: bool) -> bool {
    supported && package_beside_exe
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

/// **Whether this process has registered the package since it started, and
/// therefore whether Explorer may not have noticed yet** (§7.4b).
///
/// `SHChangeNotify(SHCNE_ASSOCCHANGED, …)` goes out on every write
/// ([`bt_platform::changing_explorer_menu`]) and it is what the documentation
/// gives for a newly registered handler. What no documentation promises is that
/// it reloads the **packaged** verb list a Windows 11 first page is drawn from:
/// that list is the App Model's rather than the class store's, and the reports
/// that exist — Windows Terminal's own, which ships this exact extension —
/// describe a running `explorer.exe` that shows a freshly registered item late
/// or not at all until it is restarted.
///
/// So the honest thing is neither to promise it worked nor to restart somebody
/// else's shell for them: it is to say, on the one row that is about this and
/// only while this process is the one that made the change, what to do if the
/// item is not there. A flag that lived past the process would be a sentence
/// nobody could ever clear.
///
/// **It goes down again on a removal that works**, so what it means is "this
/// process registered the package and has not since taken it back". A flag that
/// only ever went up would leave the row telling somebody who has just switched
/// this off that Folio is registered for the first page.
static REGISTERED_HERE: AtomicBool = AtomicBool::new(false);

/// Whether a registration made in this session may still be waiting for
/// Explorer to notice — see [`REGISTERED_HERE`].
#[must_use]
pub fn shell_refresh_pending() -> bool {
    REGISTERED_HERE.load(Ordering::Acquire)
}

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
    classify(
        registered.full_name,
        registered.external_path,
        here.as_deref(),
    )
}

#[cfg(not(windows))]
fn read_state() -> PackageState {
    PackageState::Unsupported
}

/// **Which of ours a registration is, from the folder it serves** — the whole of
/// the row's reading of the deployment database, as a function of its inputs so
/// a test can ask about a machine it is not running on.
///
/// `external` is `Package::EffectiveExternalPath`
/// ([`bt_platform::msix::registered`]). That is the **API** and not the
/// deployment service's own bookkeeping: the same folder is written down as
/// `PackageRootFolder` under
/// `HKCU\…\AppModel\Repository\Packages\<full name>`, and reading it there
/// would be reading a private store on a promise nobody made. Both were checked
/// against each other on the machine this was written on and they agree.
///
/// **`InstallLocation` is not asked about at all.** A sparse package is staged
/// under `C:\Program Files\WindowsApps\…` like every other one — that folder
/// holds the manifest, the block map and the signature, and it holding no
/// `folio.exe` is the *normal* shape of a package whose content is external
/// rather than evidence that something went wrong. A reading that took it for
/// evidence would call a perfectly good registration broken.
///
/// So there are two answers and not three. The registration serves a folder;
/// it is ours when that folder is this one and somebody else's copy's when it
/// is not. **A registration with no external location at all counts as ours**,
/// which is the conservative direction: the only thing this state buys is the
/// silent re-registration in [`begin_probe`], and re-registering over an answer
/// Windows would not give is a three-second deployment at every launch.
#[must_use]
pub fn classify(full_name: String, external: Option<PathBuf>, here: Option<&Path>) -> PackageState {
    match (external, here) {
        (Some(at), Some(here)) if !same_folder(&at, here) => {
            PackageState::Elsewhere { full_name, at }
        }
        _ => PackageState::Current { full_name },
    }
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
                    REGISTERED_HERE.store(true, Ordering::Release);
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

/// Register the package, or take it back off — half of what the row's third
/// answer means.
///
/// Returns whether a job was started. `false` is a press with nothing to do:
/// another job is already running.
///
/// **No pre-flight refusal for a missing `folio.msix`** since the ruling of
/// 2026-09-07: the answer that would need it is greyed on a machine that has
/// none, with the reason on the row's own line, so a card saying `folio.msix is
/// not beside folio.exe` would repeat a sentence the reader is looking at in
/// order to explain a press they could not have made. The sentence stays for the
/// one case that is still real — the file removed between the frame that offered
/// the answer and the thread that acts on it.
pub fn request(install: bool) -> bool {
    if BUSY.swap(true, Ordering::AcqRel) {
        return false;
    }
    let package = package_file();
    std::thread::spawn(move || {
        let outcome = if install {
            match package.as_deref().and_then(|package| {
                // The file, and the folder to register it against. Either being
                // absent is the same answer, so they are fetched as one.
                Some((package, package.parent()?))
            }) {
                Some((package, here)) => msix::register(package, here).map(|()| {
                    // The row's line says what to do if Explorer has not
                    // noticed — see `REGISTERED_HERE`. Set on the way out of a
                    // registration that worked and never on one that did not,
                    // because a refusal leaves the menu exactly as it was.
                    REGISTERED_HERE.store(true, Ordering::Release);
                    true
                }),
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
                Some(full_name) => msix::remove(full_name).map(|()| {
                    // **And the sentence goes away with the registration it was
                    // about.** `REGISTERED_HERE` means "this process registered
                    // the package and has not since taken it back"; a flag that
                    // only ever went up would leave the row telling a reader who
                    // just switched this off that Folio is registered for the
                    // first page.
                    REGISTERED_HERE.store(false, Ordering::Release);
                    false
                }),
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

/// **What On does on this machine** — the row's own sentence (user ruling
/// 2026-09-07).
///
/// A function rather than a table entry, on `context_menu::row_description`'s
/// footing: which of the four sentences is true depends on the world, and the
/// module that owns the world is the one that should be asked.
///
/// The switch has one label on every machine and means a different amount on
/// each of them, so this line is where the difference is written. It is the
/// same job the line did while the top answer could be greyed —
/// `psreadline::row_description`'s idiom, a control's own line carrying what
/// the control cannot say — asked now of a switch that is never refused.
#[must_use]
pub fn row_description() -> &'static str {
    description_for(
        supported(),
        package_file().is_some(),
        matches!(state(), PackageState::Elsewhere { .. }),
        shell_refresh_pending(),
    )
    .text()
}

/// The same answer from the four facts, so a test can ask for a machine it is
/// not running on.
///
/// **Windows' absence is reported before the file's**, because on a Windows 10
/// the file may well be sitting in the folder and naming it would answer a
/// question that machine cannot ask.
///
/// **And a registration made in this session is reported last of all** — after
/// the two that say the first page is out of reach, because on those machines
/// nothing was registered, and after the moved folder, because that one names a
/// condition the reader can still see. It is the only one of the four that is
/// about *this run of Folio* rather than about the machine, and it is the only
/// one that is advice rather than a fact: see [`shell_refresh_pending`].
#[must_use]
pub fn description_for(
    supported: bool,
    package_beside_exe: bool,
    elsewhere: bool,
    refresh_pending: bool,
) -> Text {
    if !supported {
        // **This machine has one menu, and the sentence names no page at all.**
        // Not the `Show more options` line with a caveat: that item is not on
        // this Windows, so a line naming it would send the reader looking for a
        // door that is not on their screen. What On does here is the whole of
        // what On can do here, and there is nothing missing to explain.
        return Text::DescExplorerMenuNoFirstPage;
    }
    if !package_beside_exe {
        return Text::DescExplorerMenuNoPackage;
    }
    if elsewhere {
        return Text::DescExplorerFirstPageElsewhere;
    }
    if refresh_pending {
        return Text::DescExplorerFirstPageAwaitingShell;
    }
    Text::DescExplorerMenu
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

    /// RED (user ruling 2026-09-07) — **the four states the two switches could
    /// leave a machine in, and the one answer each of them reads as.**
    ///
    /// This is the table the ruling wrote as a migration. There was nothing to
    /// migrate — **neither switch ever stored anything**: the classic half was
    /// read off `HKCU\Software\Classes` and the first-page half off the
    /// deployment database, on this dialog's rule that a machine fact gets no
    /// second copy in `settings.json` (`SettingsRow::ContextMenu`'s own note, and
    /// `bt_persist` has no key for either). So the table is applied to the
    /// machine, where the states actually live, and it is applied on every frame
    /// rather than once at a schema bump — which is strictly stronger: a package
    /// removed from `Settings ▸ Apps ▸ Installed apps` moves this row on the next
    /// draw, and no migration could have promised that.
    ///
    /// The row that needs stating is the third: a package registered with no
    /// classic trees is a state an older build's two switches could be left in,
    /// and it reads as the first page, because that is where the verb is — and
    /// the switch over it therefore reads `On`, with the `Off` beside it clearing
    /// both.
    ///
    /// MUTATION: test `classic` before `package_registered` and the third row
    /// goes red — a machine with an item on the first page would say the verb is
    /// under `Show more options`, and pressing that answer would remove nothing.
    /// Make [`ExplorerPlace::on`] answer `matches!(self, Self::FirstPage)` and the
    /// switch block goes red on the most ordinary machine there is.
    #[test]
    fn every_state_two_switches_could_leave_reads_as_one_of_three_places() {
        // classic, package  →  the place the verb stands in
        assert_eq!(place(true, true), ExplorerPlace::FirstPage);
        assert_eq!(place(true, false), ExplorerPlace::ShowMoreOptions);
        assert_eq!(place(false, true), ExplorerPlace::FirstPage);
        assert_eq!(place(false, false), ExplorerPlace::Off);
        // And the switch over it is on wherever the verb is in that menu, which
        // is either registration and not the classic one alone.
        assert!(place(true, true).on());
        assert!(place(true, false).on());
        assert!(place(false, true).on());
        assert!(!place(false, false).on());
        // The place says what it wants of each store, so the press that follows
        // puts the machine where the row says it is.
        for (classic, package) in [(true, true), (true, false), (false, false)] {
            let place = place(classic, package);
            assert_eq!(place.classic(), classic);
            assert_eq!(place.package(), package);
        }
    }

    /// RED (user ruling 2026-09-07) — **the first page needs both halves, and
    /// the row's own line says what On does without it.**
    ///
    /// Two machine facts, either of which alone makes the same item unregistrable:
    /// a Windows with no first page to reach, and a folder with no `folio.msix`
    /// to register. Together they are the whole of what
    /// [`place_when_on`] asks — which is why they are one function and not two
    /// conditions written out at each of its callers.
    ///
    /// **And there is no card for a refusal, because nothing is refused.** The
    /// switch turns on either way; a toast reading `folio.msix is not beside
    /// folio.exe` would explain a press that succeeded. That was the earlier
    /// ruling's reason for retiring it and the switch's reason for keeping it
    /// retired.
    ///
    /// MUTATION: make `first_page_offered` answer `supported || package` and the
    /// second assertion goes red, which is `On` trying to register a file that is
    /// not there. Make `place_when_on` answer `Off` for a machine with no package
    /// and the fifth goes red, which is a switch that turns on and does nothing.
    #[test]
    fn the_first_page_needs_both_halves_and_on_gives_what_is_left() {
        assert!(first_page_offered(true, true));
        assert!(!first_page_offered(true, false));
        assert!(!first_page_offered(false, true));
        assert!(!first_page_offered(false, false));
        // What On means on each of them — never nothing.
        assert_eq!(place_when_on(true), ExplorerPlace::FirstPage);
        assert_eq!(place_when_on(false), ExplorerPlace::ShowMoreOptions);
        assert!(place_when_on(true).on() && place_when_on(false).on());
        // And the card's switch is the same switch, through the same function
        // rather than through a second table saying the same thing.
        assert_eq!(
            crate::first_run::ExplorerShape::FirstPageAndClassic.place(),
            place_when_on(true)
        );
        assert_eq!(
            crate::first_run::ExplorerShape::ClassicOnly.place(),
            place_when_on(false)
        );
        // The sentence under the title says what On does here, and Windows'
        // absence is reported before the file's: on a Windows 10 the file may
        // well be there and naming it would answer a question nobody asked.
        assert_eq!(
            description_for(false, false, false, false),
            Text::DescExplorerMenuNoFirstPage
        );
        assert_eq!(
            description_for(false, true, false, false),
            Text::DescExplorerMenuNoFirstPage
        );
        assert_eq!(
            description_for(true, false, false, false),
            Text::DescExplorerMenuNoPackage
        );
        assert_eq!(
            description_for(true, true, true, false),
            Text::DescExplorerFirstPageElsewhere
        );
        assert_eq!(
            description_for(true, true, false, false),
            Text::DescExplorerMenu
        );
    }

    /// RED (user ruling 2026-09-07) — **the card names the place, and there are
    /// exactly three of them.**
    ///
    /// One press, one card. A reader who moves from the first page down to the
    /// classic entry is owed "the verb is under Show more options" rather than
    /// "the first page lost something", which is why the fourth sentence — the
    /// first page's own removal card — retired with the second row.
    ///
    /// MUTATION: answer `ExplorerFirstPageAddedToast` for `ShowMoreOptions` and
    /// the second assertion goes red, which is a card claiming the verb is on a
    /// page the press just took it off.
    #[test]
    fn one_press_raises_one_card_and_it_names_where_the_verb_ended_up() {
        for pending in [false, true] {
            assert_eq!(
                place_toast(ExplorerPlace::Off, pending),
                Text::ContextMenuRemovedToast
            );
            assert_eq!(
                place_toast(ExplorerPlace::ShowMoreOptions, pending),
                Text::ContextMenuAddedToast
            );
        }
        assert_eq!(
            place_toast(ExplorerPlace::FirstPage, false),
            Text::ExplorerFirstPageAddedToast
        );
    }

    /// RED (2026-09-07, the shell-refresh finding) — **a registration made by
    /// this process says so, on the row's line and on the card.**
    ///
    /// The bug this is the tail of: `folio.msix` was registered for this
    /// account, the deployment database agreed, the package's root folder was
    /// this executable's own folder — and Explorer's first page carried no
    /// Folio. Nothing had told the shell anything had changed
    /// (`SHChangeNotify` appears nowhere in the two releases before this one),
    /// and a running `explorer.exe` had been up since before the registration.
    ///
    /// The call now goes out on every write
    /// ([`bt_platform::changing_explorer_menu`]), and it is what the
    /// documentation gives for a newly registered handler. What it is **not**
    /// documented to do is reload the packaged verb list a Windows 11 first page
    /// is drawn from, and the reports that exist — Windows Terminal's own, which
    /// ships this same extension — are of an item that shows up late or not
    /// until Explorer restarts. So the surfaces say the one thing that always
    /// works, and they say it only while this process is the one that changed
    /// the machine.
    ///
    /// **The plain card is still reachable**, which is the assertion above: a
    /// press on a machine whose package was registered before this window opened
    /// deploys nothing, so there is nothing for the shell to have missed.
    ///
    /// MUTATION: answer `ExplorerFirstPageAddedToast` for a pending refresh and
    /// the first block goes red — the card claims the item is on the first page
    /// on the machine where it most likely is not yet. Return
    /// `DescExplorerMenu` from `description_for` for a pending refresh and the
    /// second goes red, which is the row saying nothing at all to the one reader
    /// who is staring at a menu with no Folio in it.
    #[test]
    fn a_registration_made_here_says_explorer_may_not_have_caught_up() {
        use crate::i18n::Lang;
        assert_eq!(
            place_toast(ExplorerPlace::FirstPage, true),
            Text::ExplorerFirstPageAddedRestartToast
        );
        assert_eq!(
            description_for(true, true, false, true),
            Text::DescExplorerFirstPageAwaitingShell
        );
        // And it is the last of the four to be reported: the two that say the
        // first page is out of reach registered nothing, and the moved folder
        // names a condition the reader can still act on.
        assert_eq!(
            description_for(false, true, false, true),
            Text::DescExplorerMenuNoFirstPage
        );
        assert_eq!(
            description_for(true, false, false, true),
            Text::DescExplorerMenuNoPackage
        );
        assert_eq!(
            description_for(true, true, true, true),
            Text::DescExplorerFirstPageElsewhere
        );
        // Both sentences carry the action, which is the whole of what they are
        // for. Neither says what Folio did about it, because a reader looking at
        // a menu with nothing in it is not asking that.
        let line = Text::DescExplorerFirstPageAwaitingShell.in_lang(Lang::English);
        assert!(line.contains("sign out"), "{line:?}");
        let card = Text::ExplorerFirstPageAddedRestartToast.in_lang(Lang::English);
        assert!(card.contains("restart"), "{card:?}");
    }

    /// RED (2026-09-07) — **which folder a registration serves is the whole of
    /// what the row reads, and it is the external location.**
    ///
    /// The state the diagnosis of this bug first reached for was a third one —
    /// "App Installer put a full package on the machine" — read off an
    /// `InstallLocation` under `C:\Program Files\WindowsApps`. That reading is
    /// wrong and this test is where it is refused: a sparse package **is**
    /// staged there, holding its manifest, its block map and its signature and
    /// no `folio.exe`, and that is its normal shape rather than evidence of
    /// anything. What says whether a registration is ours is the folder it
    /// points its content at, and on the machine this was written on that folder
    /// agreed with `PackageRootFolder` in the deployment service's own
    /// repository key.
    ///
    /// MUTATION: read the absent external location as anything but ours and the
    /// last assertion goes red, which is a launch that re-registers a package it
    /// had no evidence was wrong — a three-second deployment at every start.
    /// Compare the two folders with `==` and the second goes red.
    #[test]
    fn a_registration_is_ours_when_the_folder_it_serves_is_this_one() {
        const NAME: &str = "WeiyiShi.Folio_0.2.2.0_x64__cffndppawf746";
        let here = Path::new(r"E:\Programs\Folio\dist\next45");
        assert_eq!(
            classify(NAME.to_owned(), Some(here.to_path_buf()), Some(here)),
            PackageState::Current {
                full_name: NAME.to_owned()
            }
        );
        assert_eq!(
            classify(
                NAME.to_owned(),
                Some(PathBuf::from(r"e:\programs\folio\dist\next45\")),
                Some(here)
            ),
            PackageState::Current {
                full_name: NAME.to_owned()
            }
        );
        assert_eq!(
            classify(
                NAME.to_owned(),
                Some(PathBuf::from(r"C:\Tools\folio")),
                Some(here)
            ),
            PackageState::Elsewhere {
                full_name: NAME.to_owned(),
                at: PathBuf::from(r"C:\Tools\folio"),
            }
        );
        // A staging folder under `WindowsApps` is not an answer to this
        // question, and neither is the operating system declining to give one.
        assert_eq!(
            classify(NAME.to_owned(), None, Some(here)),
            PackageState::Current {
                full_name: NAME.to_owned()
            }
        );
    }

    /// RED (user ruling 2026-09-07, the second of that day) — **the row's line
    /// says what On does on this machine, one sentence per machine.**
    ///
    /// A switch has one word on every desk and buys a different amount on each,
    /// so this line is where the difference is written. Three machines, three
    /// sentences, and each of them a statement about what a press will do rather
    /// than about what some answer would have cost.
    ///
    /// **The Windows 10 line names no page at all.** Not `Show more options`
    /// with a footnote: that item is a Windows 11 item, and a line naming it on a
    /// machine that has one menu would send the reader looking for a door that is
    /// not drawn on their screen. The line that used to stand here said the
    /// opposite thing — that this Windows has no first page — which was the
    /// reason a rung was greyed, and there is no rung now.
    ///
    /// MUTATION: return `DescExplorerMenuNoPackage` for a machine with no first
    /// page and the first assertion goes red, which is a Windows 10 reader told
    /// to go and find a file for a page their Windows does not have.
    #[test]
    fn the_row_says_what_on_does_on_this_machine() {
        use crate::i18n::Lang;
        let ten = description_for(false, true, false, false).in_lang(Lang::English);
        assert!(
            !ten.contains("first page"),
            "a machine with one menu is told about one menu: {ten:?}"
        );
        assert!(!ten.contains("Show more options"), "{ten:?}");
        let no_package = description_for(true, false, false, false).in_lang(Lang::English);
        assert!(no_package.contains("Show more options"), "{no_package:?}");
        assert!(
            no_package.contains("folio.msix is not in this folder"),
            "{no_package:?}"
        );
        let both = description_for(true, true, false, false).in_lang(Lang::English);
        assert!(both.contains("first page"), "{both:?}");
        assert!(both.contains("Show more options"), "{both:?}");
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
