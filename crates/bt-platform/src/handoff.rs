//! **Everything this product gives to the machine, and the one place that
//! decides what a real target looks like.**
//!
//! Four verbs leave this window and end up somewhere Folio no longer controls:
//! open a file with whatever handler is registered for it, show a file in
//! Explorer, open an address in the reader's own browser, and start a helper
//! program to ask it a question. Each of them used to carry its own reading of
//! the string it was handed, and the readings did not agree with each other or
//! with Windows:
//!
//! * the reveal wrapped whatever arrived in quotes and asked nothing else, so a
//!   name that closed the quote became a second argument on Explorer's command
//!   line and a path that named nothing opened a folder nobody asked for;
//! * the program refusal read the extension off the name **as it arrived**
//!   while the call passed the same name to `ShellExecuteW`, which strips
//!   trailing dots and spaces before it resolves anything — so `payload.exe.`
//!   was refused as a document and started as a program;
//! * the helpers named their programs by bare name, and both `CreateProcess`
//!   and `cmd /c` look in the process's working directory before they look at
//!   `PATH`.
//!
//! So this module is the whole of the hand-off: it **normalises the target the
//! way Windows will**, refuses the shapes a real target never has, names every
//! program by an absolute path found somewhere an administrator put it, gives
//! every launch an explicit working directory, and holds the only
//! `ShellExecuteW` call sites in the workspace. The pure half — what a name
//! resolves to, what may be handed to Explorer, where a program is looked for —
//! is a set of ordinary functions with ordinary tests, because the one thing
//! that can be wrong here is a string and a string can be shot at without
//! launching anything.
//!
//! **And on macOS it is the same module with a second shell behind it** (M2-2).
//! `macos_handoff` holds the only `NSWorkspace` hand-off in the workspace for
//! the same reason `windows_handoff` holds the only `ShellExecuteW`: the four
//! verbs are one decision about what a real target is, and the decision does
//! not become four decisions because there are two machines. What it does
//! become is **two readings of a path**, and that is the one thing the crossing
//! genuinely changes — Win32's grammar (drive letters, `PATHEXT`, the
//! trailing-dot trim) versus POSIX's (bytes, a leading `/`, the execute bit) —
//! so each arm states its own and neither borrows the other's.
//!
//! **And none of it runs on the window thread** (`docs/DESIGN.md`, 2026-09-22 —
//! *a hand-off to the system runs on its own lane*). Every door here is still
//! synchronous and still answers a real `Result`; what moved is who waits for
//! it. `bt-app`'s OS hand-off lane is one below-normal thread that enters a
//! [`ShellThread`] once and hands each [`Handoff`] to the door it names, in the
//! order the reader pressed, and the window hears the answer later through its
//! event loop. The doors themselves did not move and did not change what they
//! decide: [`ShellThread::hand_over`] is a `match` onto the same functions the
//! window thread used to call.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::NativeWindow;
use crate::admission::WorkerCtx;

/// The refusal's own words, so the caller can tell "this window will not do
/// that" apart from "Windows could not".
pub const PROGRAM_REFUSED: &str = "the files tree does not run programs";

/// **What a worker already established about a path** — the two facts a reveal needs, carried
/// instead of fetched (closure re-review B-1').
///
/// `bt-term`'s ledger is the authority (`bt_term::PathVerdict`) and it cannot be named here,
/// because that crate depends on this one and not the reverse. So the two fields travel as a type
/// of this crate's own, and the door refuses on `exists` itself rather than trusting the caller to
/// have checked: the first shape of this door took a lone `bool` and every caller forgot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedTarget {
    /// The disk said something is there under that name.
    pub exists: bool,
    /// And that it is a folder.
    pub is_directory: bool,
    /// And that opening it would **run** it — the execute bit, or an application bundle's own
    /// name. Always `false` on Windows, where that question is about the association and
    /// [`names_a_program`] answers it from the name.
    pub executable: bool,
    /// **The finished name the door hands over**, [`resolved_for_a_door`]'s answer — and `None`
    /// when the platform would not give one, in which case a door uses the printed spelling,
    /// which is what it had before there was a resolver in front of it.
    pub resolved: Option<PathBuf>,
}

impl VerifiedTarget {
    /// **The answer for a name nobody has asked the disk about** — every door refuses it.
    #[must_use]
    pub const fn absent() -> Self {
        Self {
            exists: false,
            is_directory: false,
            executable: false,
            resolved: None,
        }
    }
}

/// **One hand-off, as a value** — what the OS hand-off lane carries
/// (`docs/ARCHITECTURE.md` §5.1).
///
/// One variant per door, holding exactly the arguments that door takes after
/// the window, so a request is the call written down rather than a second
/// description of it: whatever a door decides — the normalisation, the program
/// refusal, the reveal's one token — it decides on the lane from the same
/// arguments it was given on the window thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Handoff {
    /// [`open_local_path`] — a file the user picked, to its registered handler.
    Open(PathBuf),
    /// [`open_local_path_verified`] — a printed reference the pane's ledger answered for.
    OpenVerified(PathBuf, VerifiedTarget),
    /// [`reveal_in_explorer`] — a path shown in the file manager.
    Reveal(PathBuf),
    /// [`reveal_verified`] — the same, for a path the ledger answered for.
    RevealVerified(PathBuf, VerifiedTarget),
    /// [`shell_execute`] — an address that has already passed the caller's policy: the web
    /// through `webnav::address_bar`, and since ticket 14 a `Ctrl`+click on a URI of any other
    /// scheme, which the owner's ruling of 2026-09-21 admits whole.
    Address(String),
    /// [`open_local_file`] — a decoded local picture, to the system viewer.
    LocalImage(PathBuf),
    /// [`open_system_fonts_page`] — the system's own fonts page.
    FontsPage,
}

/// **The thread the hand-offs run on, prepared for them** — the proof a
/// [`Handoff`] is only handed over from a thread that has been made ready.
///
/// On Windows that preparation is COM. `ShellExecuteW` can delegate to shell
/// extensions that are activated through COM, and Microsoft's own instruction
/// for any caller is `CoInitializeEx(COINIT_APARTMENTTHREADED |
/// COINIT_DISABLE_OLE1DDE)` on the calling thread first. The window thread had
/// that for free — winit initialises OLE there for drag and drop — and the lane
/// is a fresh thread with nothing, so it enters its apartment here, once, and
/// leaves it when the value is dropped. Not `Send`: an apartment belongs to the
/// thread that entered it, and the type says so.
///
/// On macOS it is an autorelease pool per hand-off (`NSWorkspace` answers with
/// autoreleased objects, and a thread of our own has no pool that drains
/// between requests); on a third platform it is nothing, and every door there
/// already refuses.
///
/// **And it is a worker's** (A1b). [`ShellThread::enter`] takes the
/// [`WorkerCtx`] the thread door lends the body of every thread it starts, so a
/// hand-off can be prepared only on a thread the door started — never on the
/// window thread, which has no such value, and never on a callback thread.
///
/// It stays on the thread that entered it — its auto traits, each probed alone:
///
/// ```compile_fail
/// fn is_send<T: Send>() {}
/// is_send::<bt_platform::ShellThread>();
/// ```
///
/// ```compile_fail
/// fn is_sync<T: Sync>() {}
/// is_sync::<bt_platform::ShellThread>();
/// ```
///
/// MUTATION: make `_not_send` a `PhantomData<()>` and both compile. The control
/// instantiates the same probes at a byte:
///
/// ```
/// fn is_send<T: Send>() {}
/// fn is_sync<T: Sync>() {}
/// is_send::<u8>();
/// is_sync::<u8>();
/// ```
///
/// — and it cannot be carried to another thread, with no `'static` bound in the
/// way (a scoped thread):
///
/// ```compile_fail
/// // RED (A1b) — an entered apartment does not move to a thread that did not enter it.
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |ctx| {
///         let shell = bt_platform::ShellThread::enter(ctx);
///         std::thread::scope(|threads| {
///             threads.spawn(move || drop(shell));
///         });
///     },
/// );
/// ```
///
/// MUTATION: make `_not_send` a `PhantomData<()>` and it compiles. The control
/// is the same scoped spawn carrying a byte, the value dropped where it was
/// made:
///
/// ```no_run
/// let _ = bt_platform::spawn_at_priority(
///     "doc-probe",
///     bt_platform::ThreadPriority::BelowNormal,
///     |ctx| {
///         let shell = bt_platform::ShellThread::enter(ctx);
///         let byte = 0_u8;
///         std::thread::scope(|threads| {
///             threads.spawn(move || drop(byte));
///         });
///         drop(shell);
///     },
/// );
/// ```
pub struct ShellThread {
    /// Whether this value entered an apartment and so owes the matching leave.
    #[cfg(windows)]
    entered: bool,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl ShellThread {
    /// Prepare the calling thread for hand-offs. Call it on the lane thread,
    /// before the first request.
    ///
    /// `worker` is the capability the thread door lent this thread's body: the
    /// signature is the whole of the check, and the value is not read.
    #[must_use]
    pub fn enter(worker: &WorkerCtx) -> Self {
        let _ = worker;
        Self {
            #[cfg(windows)]
            entered: windows_handoff::enter_apartment(),
            _not_send: std::marker::PhantomData,
        }
    }

    /// **Hand one request to the door it names**, with `window` — the window
    /// that asked — as that door's window argument.
    ///
    /// The answer is the door's own, byte for byte: `Ok` when the system took
    /// it, the door's refusal otherwise.
    ///
    /// # Errors
    ///
    /// Whatever the named door refuses with.
    pub fn hand_over(&self, window: NativeWindow, request: &Handoff) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            objc2::rc::autoreleasepool(|_| Self::door(window, request))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::door(window, request)
        }
    }

    fn door(window: NativeWindow, request: &Handoff) -> Result<(), String> {
        match request {
            Handoff::Open(path) => open_local_path(window, path),
            Handoff::OpenVerified(path, target) => {
                open_local_path_verified(window, path, target.clone())
            }
            Handoff::Reveal(path) => reveal_in_explorer(window, path),
            Handoff::RevealVerified(path, target) => reveal_verified(window, path, target.clone()),
            Handoff::Address(target) => shell_execute(window, target),
            Handoff::LocalImage(path) => open_local_file(window, path),
            Handoff::FontsPage => open_system_fonts_page(window),
        }
    }
}

impl Drop for ShellThread {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.entered {
            windows_handoff::leave_apartment();
        }
    }
}

/// The extensions that are a program whatever this machine's `PATHEXT` says.
///
/// `PATHEXT` is the system's own list of what a *command line* will execute
/// and it is read as well, but it is not the whole answer: a `.lnk` is not
/// on it and points at anything at all, a `.scr` is an executable wearing a
/// screensaver's name, and `.hta`, `.reg`, `.msi` and `.url` are each opened
/// by a handler whose whole job is to act. Reading both means the list grows
/// with a machine that has added to `PATHEXT` without shrinking on one that
/// has emptied it.
const ALWAYS_A_PROGRAM: &[&str] = &[
    "appref-ms",
    "bat",
    "cmd",
    "com",
    "cpl",
    "exe",
    "hta",
    "jar",
    "js",
    "jse",
    "lnk",
    "msc",
    "msi",
    "msp",
    "ps1",
    "pif",
    "reg",
    "scf",
    "scr",
    "url",
    "vb",
    "vbe",
    "vbs",
    "wsf",
    "wsh",
];

/// The extensions `PATHEXT` is taken to hold when the machine has not said.
///
/// The documented default. It is here rather than at the one call site because
/// a search that found nothing because a variable was missing is a search that
/// would then have fallen back to whatever the working directory holds, which
/// is the thing this module exists to refuse.
///
/// `#[cfg(windows)]` with the search that reads it. On Unix the question this
/// answers — which spellings of a bare name are a program — is the execute bit
/// and not an extension list; `macos_handoff::opening_it_would_run_it` is where
/// that question is really asked, and it is asked of a file rather than of a
/// name.
#[cfg(windows)]
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// **The final component of a path as Windows itself will resolve it.**
///
/// Win32 strips trailing dots and spaces from a component before it opens
/// anything, which is why `payload.exe.` and `payload.exe ` are two spellings
/// of one file. Every question this module asks about a *name* — is it a
/// program, what is its extension — is asked of this answer, and the call that
/// follows is handed this answer too, so the check and the call cannot be
/// talking about two different files.
///
/// Both separators, because a `file:` URI decodes to forward slashes and
/// Windows takes either.
#[must_use]
pub fn effective_final_component(path: &Path) -> String {
    let text = path.as_os_str().to_string_lossy();
    let component = text.rsplit(['\\', '/']).next().unwrap_or_default();
    component.trim_end_matches(['.', ' ']).to_owned()
}

/// The same path with its final component spelled the way Windows will resolve
/// it, or nothing when the trim leaves no name at all.
///
/// `C:\bin\...` is a folder named by three dots to a person and is nothing to
/// Windows, so it is not a target.
#[must_use]
pub fn normalised_target(path: &Path) -> Option<PathBuf> {
    let name = effective_final_component(path);
    if name.is_empty() {
        return None;
    }
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Some(parent.join(name)),
        // A drive root has no parent and no name to trim: `C:\` is already
        // itself.
        _ => Some(path.to_path_buf()),
    }
}

/// Whether this spelling asks Windows to skip its own normalisation.
///
/// `\\?\` and `\\.\` go to the object manager with the text intact — no
/// trailing-dot trim, no `..` folding, no reserved-name rule — so a path
/// wearing one is a path where [`effective_final_component`] would be answering
/// about a different file than the one that opens. `\??\` is the same door
/// spelled the way the native API spells it. None of the three is ever
/// produced by a files column, a git pathname, an OSC 8 target or a person
/// typing, so all three are refused rather than reasoned about.
#[must_use]
pub fn asks_windows_not_to_normalise(path: &Path) -> bool {
    let text = path.as_os_str().to_string_lossy().replace('/', "\\");
    text.starts_with(r"\\?\") || text.starts_with(r"\\.\") || text.starts_with(r"\??\")
}

/// Whether opening this name would start something rather than show it.
///
/// Given `pathext` as an argument rather than reading the environment itself,
/// so the rule is answerable in a test on a machine whose own `PATHEXT` is
/// whatever it is.
///
/// The extension is read off [`effective_final_component`] and not off
/// `Path::extension`, which is the whole of R1-11: the two disagree exactly
/// where it matters, and the one that agrees with `ShellExecuteW` is this one.
/// A name that is nothing but an extension (`.exe`) counts as that extension,
/// because that is what Windows does with it.
#[must_use]
pub fn names_a_program(path: &Path, pathext: &str) -> bool {
    let name = effective_final_component(path);
    let Some((_, extension)) = name.rsplit_once('.') else {
        // No extension means no registered handler to speak of, and
        // `ShellExecute` falls back to the "open with" chooser rather than
        // to running anything. That is a dialog, not an execution.
        return false;
    };
    if extension.is_empty() {
        return false;
    }
    let extension = extension.to_ascii_lowercase();
    ALWAYS_A_PROGRAM.contains(&extension.as_str())
        || pathext.split(';').any(|entry| {
            let entry = entry.trim().trim_start_matches('.');
            !entry.is_empty() && entry.eq_ignore_ascii_case(&extension)
        })
}

/// The shape gate every path this module hands to the shell keeps: absolute,
/// nameable, and spelled the way Windows reads paths.
///
/// Wider than [`validate_local_image_path`] in exactly one way — a UNC share
/// is allowed — because a files column may legitimately be rooted at
/// `\\server\share`, and a tree that can list a path it then refuses to open
/// is a tree that lies about what its rows are.
pub fn validate_openable_path(path: &Path) -> Result<(), String> {
    let text = path.as_os_str().to_string_lossy();
    if text.contains('\0') {
        return Err("path contains an embedded NUL".to_owned());
    }
    if asks_windows_not_to_normalise(path) {
        return Err("path asks Windows to skip its own normalisation".to_owned());
    }
    let bytes = text.as_bytes();
    let drive_rooted = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let unc = text.starts_with(r"\\") && text.len() > 2;
    if !drive_rooted && !unc {
        return Err("path must be absolute".to_owned());
    }
    Ok(())
}

/// The narrower gate the image lane keeps: drive-rooted, and a picture.
pub fn validate_local_image_path(path: &Path) -> Result<(), String> {
    let text = path.as_os_str().to_string_lossy();
    if text.contains('\0') {
        return Err("local image path contains an embedded NUL".to_owned());
    }
    if asks_windows_not_to_normalise(path) {
        return Err("local image path asks Windows to skip its own normalisation".to_owned());
    }
    let bytes = text.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
    {
        return Err("local image path must be drive-rooted and absolute".to_owned());
    }
    let name = effective_final_component(path);
    let allowed_extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .is_some_and(|extension| crate::IMAGE_FILE_EXTENSIONS.contains(&extension.as_str()));
    if !allowed_extension {
        return Err("local image path extension is not supported".to_owned());
    }
    Ok(())
}

/// The parameters `explorer.exe` is handed to **reveal** a path (user ruling,
/// 2026-08-13), or nothing when the path is not one this window will hand over.
///
/// # 「Show me where this is」, not 「open the folder it is in」
///
/// Every foot in this window carries a path and offers to take you to it. What
/// that used to mean was `ShellExecute("open", <the parent folder>)` — Explorer
/// opened on a directory and the file the foot was actually naming was one of
/// two hundred rows, indistinguishable from the rest. `/select` is the verb
/// that means what the foot says: the folder opens **with that item
/// highlighted**.
///
/// A directory keeps the old answer, and that is a judgement rather than a
/// limitation: `/select` on a folder opens its *parent* with the folder
/// highlighted, which is one level further out than a foot pointing at a root
/// is offering. Looking *inside* it is the natural reading of a tree's own
/// root, so a directory is opened and a file is selected. **Which of the two it
/// is is read here** rather than passed in, because the caller reading the disk
/// and this function building the string are the two halves that must not
/// disagree.
///
/// # Why it asks the disk (R1-5)
///
/// Explorer parses `/select,<path>` as one token and wants the path quoted, and
/// a command line that is a quote out is a command line that silently opens
/// `Documents` instead. Quoting alone is not enough for that promise:
///
/// * a `"` inside the name closes the quoted run and hands Explorer a second
///   argument — no NTFS volume can hold such a name, which is exactly why a
///   string carrying one did not come from a file;
/// * `..` in the middle means the text and the place are two different
///   questions, and only one of them is what the foot said;
/// * a path that is not there leaves Explorer to fall back to a folder nobody
///   named, which reads as this window having opened the wrong thing rather
///   than as a refusal.
///
/// So the path is canonicalised first — which resolves `..`, settles the
/// spelling, and fails outright when there is nothing there — and the argument
/// is built from the answer the operating system gave. The `\\?\` prefix
/// `canonicalize` returns is taken back off, because Explorer does not read it.
#[must_use]
pub fn reveal_arguments(path: &Path) -> Option<OsString> {
    if validate_openable_path(path).is_err() {
        return None;
    }
    let metadata = std::fs::metadata(path).ok()?;
    let canonical = resolved_for_a_door(path)?;
    reveal_argument_form(&canonical, metadata.is_dir())
}

/// **The name a hand-off door gives the operating system** — the two lines that stood inside
/// [`reveal_arguments`] and inside both macOS doors, named once so there is one of them.
///
/// `canonicalize` resolves `..`, settles the spelling and fails outright when there is nothing
/// there; [`strip_verbatim_prefix`] takes off the verbatim prefix `canonicalize` writes on Windows
/// and that every consumer of a path — Explorer, [`validate_openable_path`],
/// [`reveal_argument_form`] — refuses. The two belong together and were separated once, on a lane: the worker produced a raw
/// canonical, the door refused its own verbatim prefix, and every `Ctrl`+click on a printed path
/// died silently (closure review r6).
///
/// It is a *disk* call and therefore a worker's, which is the whole reason it is reachable from
/// outside this module: `bt_term::verify_path` runs it there, the doors take the answer, and this
/// is the one body both read so the two cannot come to two spellings of one file.
#[must_use]
pub fn resolved_for_a_door(path: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(path).ok()?;
    Some(strip_verbatim_prefix(&canonical))
}

/// The string half of [`reveal_arguments`], over a path somebody has already
/// established is there.
///
/// Separate so that the form Explorer is handed can be pinned without a disk,
/// and so that the two refusals read as what they are: one is about the machine
/// (there is no such file), the other is about the text (Explorer would not
/// read this as one argument).
#[must_use]
pub fn reveal_argument_form(path: &Path, is_directory: bool) -> Option<OsString> {
    let text = path.as_os_str().to_string_lossy().into_owned();
    if validate_openable_path(path).is_err() {
        return None;
    }
    // A quote ends the quoted run; a control character is not in any name a
    // filesystem handed back. Either means the string did not come from a file.
    if text.contains('"') || text.chars().any(char::is_control) {
        return None;
    }
    // `..` is a text question and Explorer answers it its own way. A
    // canonicalised path has none.
    if Path::new(&text)
        .components()
        .any(|part| part.as_os_str() == "..")
    {
        return None;
    }
    let mut arguments = OsString::new();
    if !is_directory {
        arguments.push("/select,");
    }
    arguments.push("\"");
    arguments.push(&text);
    arguments.push("\"");
    Some(arguments)
}

/// `\\?\C:\x` back to `C:\x`, and `\\?\UNC\server\share` back to
/// `\\server\share` — the two prefixes `std::fs::canonicalize` writes.
///
/// Explorer, and every other shell consumer, reads the ordinary spellings. The
/// verbatim ones are refused as *input* ([`asks_windows_not_to_normalise`]);
/// this is the other direction, taking one off a string the operating system
/// itself produced.
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

/// **Where a program this product starts is looked for, and the working
/// directory is not one of the places** (R1-17).
///
/// `CreateProcess` given a bare name searches the application directory, then
/// **the current directory**, then the system directories, then `PATH`; `cmd
/// /c` given a bare name does the same. The current directory of a terminal
/// emulator is whatever folder the reader was standing in when they started it,
/// which means a `copilot.cmd` sitting in a cloned repository ran the moment
/// the settings page that asks about copilot was opened. So the search is
/// written here instead — Windows' own order with that one entry taken out —
/// and what is handed to the launcher is an absolute path with nothing left to
/// resolve.
///
/// Pure, with the directories, the extension list and the disk all passed in,
/// so the order can be pinned without a machine that has any of these programs.
///
/// A `name` that is already absolute is already an answer. A `name` carrying a
/// separator without being absolute is *relative to the working directory*,
/// which is the thing being refused, so it is not a program name at all.
#[must_use]
pub fn program_in_directories(
    name: &Path,
    directories: &[PathBuf],
    pathext: &str,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if name.is_absolute() {
        return Some(name.to_path_buf());
    }
    let spelling = name.as_os_str().to_string_lossy();
    if spelling.is_empty() || spelling.contains(['\\', '/']) {
        return None;
    }
    let mut spellings = Vec::new();
    // A name that carries its own extension is tried as it stands first, which
    // is what `where.exe` does and what a caller naming `cmd.exe` means.
    if effective_final_component(name).contains('.') {
        spellings.push(spelling.to_string());
    }
    for entry in pathext.split(';') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        // Lower case: `PATHEXT` is written in capitals and the file beside it
        // is not, and the answer this hands back is read by a person in a
        // command line. The filesystem does not care either way.
        let entry = entry
            .strip_prefix('.')
            .unwrap_or(entry)
            .to_ascii_lowercase();
        spellings.push(format!("{spelling}.{entry}"));
    }
    for directory in directories {
        for candidate in &spellings {
            let candidate = directory.join(candidate);
            if is_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// **The five verbs that leave this window, on a platform whose shell has not
/// been asked yet** (M2-2 wrote the macOS arm next door; this is what a third
/// platform still meets).
///
/// Three things are worth reading here rather than in a ticket.
///
/// **The window goes.** Every one of these took an `HWND` because
/// `ShellExecuteW` takes one — it is the window an error box would be parented
/// to. `NSWorkspace` has nothing to be given, so on macOS the parameter is
/// spare exactly as the clipboard's was (M1-9); it stays in the signature
/// because the Windows arm still needs it, and a door in this crate has one
/// signature (§4.4 ②).
///
/// **The refusal is not the same as `PROGRAM_REFUSED`.** That sentence is *this
/// window will not run programs*, a product rule the caller matches on and
/// turns into a notice in the files column; what these say is that the machine
/// was never asked. Keeping them apart is why the text below does not contain
/// it.
///
/// **The path gate above is not disabled.** `validate_openable_path` and
/// `names_a_program` still run on the caller's side of these doors, and their
/// grammar is Windows' — `PATHEXT`, drive letters, the trailing-dot trim. On
/// Unix the question "is this a program" is the execute bit and the answer is
/// a different one; `macos_handoff` states that rather than letting a Windows
/// reading of a Unix path decide anything, and until a third platform has a
/// shell backend of its own that is the second reason these refuse instead of
/// quietly calling `open`.
#[cfg(not(windows))]
mod portable_handoff {
    use std::path::{Path, PathBuf};

    #[cfg(not(target_os = "macos"))]
    use crate::NativeWindow;

    /// The sentence all five say.
    #[cfg(not(target_os = "macos"))]
    fn not_here(what: &str) -> String {
        format!("{what} is not on this platform yet")
    }

    /// **Where a bare program name resolves on `PATH`, and off Windows nowhere**
    /// — this arm's one door that macOS takes too (M2-2 looked at it and left
    /// it).
    ///
    /// The refusal is M1-10's and it is a decision rather than a gap. `PATHEXT`
    /// is the Windows spelling of "which spellings of this bare name are a
    /// program", and the POSIX question is a different one — the execute bit —
    /// so the two are not one rule with a parameter. Writing the POSIX search
    /// here would be writing a door with no caller: the one reader of this name
    /// is `attention_copilot::run_probe`, which is inside one of the eleven
    /// `#[cfg(windows)]` arms §4.3 of the plan lists, and a Mac has no copilot
    /// probe until the ticket that decides what discovering an agent means on
    /// this platform (M4-6's neighbourhood, plan §4.3). Porting the shape
    /// rather than the behaviour is the mistake the backend inventory's §5 asks
    /// the port not to make; the honest answer until then is the refusal, and
    /// `a_posix_program_is_looked_for_where_an_administrator_put_it` holds it.
    #[must_use]
    pub fn program_on_path(name: &Path) -> Option<PathBuf> {
        let _ = name;
        None
    }

    /// Open one already-policy-checked address with the system's handler.
    #[cfg(not(target_os = "macos"))]
    pub fn shell_execute(window: NativeWindow, target: &str) -> Result<(), String> {
        let _ = (window, target);
        Err(not_here("opening an address"))
    }

    /// Open one worker-validated local image with its default handler.
    #[cfg(not(target_os = "macos"))]
    pub fn open_local_file(window: NativeWindow, path: &Path) -> Result<(), String> {
        let _ = (window, path);
        Err(not_here("opening a file"))
    }

    /// Open one file a person picked out of a directory listing.
    #[cfg(not(target_os = "macos"))]
    pub fn open_local_path(window: NativeWindow, path: &Path) -> Result<(), String> {
        let _ = (window, path);
        Err(not_here("opening a file"))
    }

    /// The same, for a path a worker has already answered for.
    #[cfg(not(target_os = "macos"))]
    pub fn open_local_path_verified(
        window: NativeWindow,
        path: &Path,
        target: super::VerifiedTarget,
    ) -> Result<(), String> {
        let _ = (window, path, target);
        Err(not_here("opening a file"))
    }

    /// Show a file in the file manager.
    #[cfg(not(target_os = "macos"))]
    pub fn reveal_in_explorer(window: NativeWindow, path: &Path) -> Result<(), String> {
        let _ = (window, path);
        Err(not_here("showing a file in the file manager"))
    }

    /// The same, for a path a worker has already answered for.
    #[cfg(not(target_os = "macos"))]
    pub fn reveal_verified(
        window: NativeWindow,
        path: &Path,
        target: super::VerifiedTarget,
    ) -> Result<(), String> {
        let _ = (window, path, target);
        Err(not_here("showing a file in the file manager"))
    }

    /// Open the system's font page — Font Book, or the fonts folder.
    #[cfg(not(target_os = "macos"))]
    pub fn open_system_fonts_page(window: NativeWindow) -> Result<(), String> {
        let _ = window;
        Err(not_here("the system font settings"))
    }
}

/// **`program_on_path` has one arm off Windows and macOS takes it too**, which
/// is why it is re-exported on its own rather than with the five beside it.
#[cfg(not(windows))]
pub use portable_handoff::program_on_path;

/// The other five, on a platform with neither a Win32 shell nor a `NSWorkspace`.
#[cfg(all(not(windows), not(target_os = "macos")))]
pub use portable_handoff::{
    open_local_file, open_local_path, open_local_path_verified, open_system_fonts_page,
    reveal_in_explorer, reveal_verified, shell_execute,
};

/// **The five verbs that leave this window, over `NSWorkspace`** (M2-2).
#[cfg(target_os = "macos")]
pub use macos_handoff::{
    open_local_file, open_local_path, open_local_path_verified, open_system_fonts_page,
    reveal_in_explorer, reveal_verified, shell_execute,
};

/// **Everything this product gives to the machine, on a Mac** — the macOS twin
/// of `windows_handoff`, and the ninth unsafe boundary in this crate (M2-2).
///
/// # The thread, and why this file has no gate where `macos_impl` has one
///
/// Every door in `macos_impl` begins by proving it is on the main thread,
/// because everything in it is `NSWindow`, `NSScreen` and `NSView` — AppKit's
/// view layer, which is the main thread's or it is undefined behaviour. **These
/// doors are not that**, and the difference is not a relaxation somebody took:
/// `NSWorkspace` owns no view and draws nothing. What it does is send a request
/// to LaunchServices and, for the reveal, an Apple event to Finder; the window
/// the file opens in belongs to another process.
///
/// The citation is Apple's own, and it is a rule rather than a sentence about
/// this class. The *Thread Safety Summary* (Cocoa Multithreading Programming
/// Guide) lists the Foundation and AppKit classes that are thread-safe and the
/// ones that are not, and says of everything it does not list: **"In most
/// cases, you can use these classes from any thread as long as you use them
/// from only one thread at a time."** `NSWorkspace` is on neither list, and its
/// own class reference states no thread requirement — unlike `NSView`,
/// `NSWindow` and `NSApplication`, each of which says outright that it is the
/// main thread's. So the honest statement is the one this file makes: **there
/// is no main-thread requirement to prove here, and a gate in front of a call
/// that does not need one would be a check that proves nothing** — which is
/// what `every_macos_window_door_proves_its_thread_before_it_calls_appkit`
/// already says about the two doors in `macos_impl` that reach no AppKit.
///
/// What these calls *are* is **synchronous and blocking**: `openURL:` waits on
/// a LaunchServices round trip, which is why Apple added
/// `openURL:configuration:completionHandler:` beside it. The doors keep the
/// synchronous form, because a door whose `Result` is a real answer is worth a
/// wait — **but the wait is no longer the window's.** Until 2026-09-22 this
/// paragraph said the window thread paid it, on both arms, and called that a
/// fair price; the measured 1.4 s `Ctrl`+click stall on Windows reversed the
/// bargain (`docs/DESIGN.md`, 2026-09-22 — *a hand-off to the system runs on its
/// own lane*). Every door here is now called from `bt-app`'s OS hand-off lane
/// through [`super::ShellThread`], and the window hears the `Result` through its
/// event loop.
///
/// **No foreground code on this arm.** `openURL:` activates the application
/// that receives the URL, `activateFileViewerSelectingURLs:` activates Finder
/// (it is documented as doing so), and `open_folder_in_finder` activates Finder
/// itself — so the receiver comes to the front without this process granting
/// anything, and macOS has no foreground lock to grant against.
///
/// # The window is spare, and stays in the signature
///
/// Each of these takes a `NativeWindow` because `ShellExecuteW` once took the
/// asking window as its `HWND` — the window an error box is parented to. Neither
/// arm parents anything to it now: `NSWorkspace` has nothing to be given one,
/// and the Windows arm hands `ShellExecuteW` no owner since the call left the
/// window thread (see `windows_handoff::hand_over`). The parameter stays because
/// a door in this crate has one signature on every platform (§4.4 ②, M1-9) and
/// it names the window that asked; it is consumed with `let _ = window;` at the
/// top of each body so that a reader meets the fact rather than deducing it.
///
/// # The path gate is this platform's, not Windows'
///
/// [`validate_openable_path`] wants a drive letter or a UNC share and
/// [`names_a_program`] reads `PATHEXT`. Neither is a question about a Unix
/// path, so neither is asked here; `openable_unix_path` and
/// `opening_it_would_run_it` are, and they are two paragraphs rather than two
/// translations. See each for what it decided and what it costs.
#[cfg(target_os = "macos")]
mod macos_handoff {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSApplicationActivationOptions, NSRunningApplication, NSWorkspace,
        NSWorkspaceOpenConfiguration,
    };
    use objc2_foundation::{NSArray, NSError, NSString, NSURL, ns_string};

    use super::PROGRAM_REFUSED;
    use crate::NativeWindow;
    use crate::macos_files::file_url;

    /// **The shape gate every path this module hands to the workspace keeps**,
    /// and it is three lines because a POSIX path has three ways of not being
    /// one.
    ///
    /// The Windows twin, [`super::validate_openable_path`], is long because
    /// Win32 has a path *grammar*: drive letters, UNC shares, verbatim prefixes
    /// that turn normalisation off, a trailing-dot trim that makes two
    /// spellings one file. None of that exists here. A POSIX path is bytes with
    /// `/` between them, it is absolute when the first byte is `/`, and the one
    /// byte it may never contain is NUL — because that is the terminator of the
    /// C string the kernel is handed, so a path carrying one would reach the
    /// file system cut short at a different file.
    ///
    /// **Relative is refused rather than resolved**, for `program_in_directories`'
    /// reason one floor down: what a relative path resolves against is this
    /// process's working directory, which is whatever folder the shell that
    /// started Folio was standing in — not something a reader pointed at.
    fn openable_unix_path(path: &Path) -> Result<(), String> {
        let bytes = path.as_os_str().as_bytes();
        if bytes.is_empty() {
            return Err("path is empty".to_owned());
        }
        if bytes.contains(&0) {
            return Err("path contains an embedded NUL".to_owned());
        }
        if !path.is_absolute() {
            return Err("path must be absolute".to_owned());
        }
        Ok(())
    }

    /// The narrower gate the image lane keeps: absolute, and a picture.
    ///
    /// The macOS reading of [`super::validate_local_image_path`], with the one
    /// clause that was Windows' — drive-rooted — replaced by this platform's
    /// answer to the same question and the extension list left exactly as it
    /// is, because that list is the decoder's and the decoder is the same
    /// program on both machines.
    fn openable_local_image(path: &Path) -> Result<(), String> {
        openable_unix_path(path).map_err(|reason| format!("local image {reason}"))?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let allowed = name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .is_some_and(|extension| crate::IMAGE_FILE_EXTENSIONS.contains(&extension.as_str()));
        if !allowed {
            return Err("local image path extension is not supported".to_owned());
        }
        Ok(())
    }

    /// **Whether opening this would run it rather than show it, on macOS.**
    ///
    /// The product rule is `DESIGN.md` §7.1.3's and it does not change at the
    /// border: *the tree is a way of looking at files, and the thing next to it
    /// that runs programs is the terminal.* What changes is the fact the rule
    /// is asked of. Windows answers with an extension list, because Windows
    /// decides what a file is by its extension and `ShellExecuteW` on a `.exe`
    /// runs it. macOS decides by type, and **opening a document here never
    /// executes it** — LaunchServices hands it to the application registered
    /// for its type. There are exactly two shapes where opening *is* running,
    /// and they are what this asks about:
    ///
    /// ① **a bundle** — a directory whose name ends in `.app`. Double-clicking
    ///    one launches it, and that is the whole of what an application is on
    ///    this platform. Other packages (`.bundle`, `.framework`,
    ///    `.qlgenerator`) are libraries something else loads; opening one in
    ///    Finder shows a folder.
    /// ② **a file with the execute bit** — which is how a Mach-O binary, a
    ///    `.command` and a shell script with no extension at all get typed
    ///    `public.unix-executable`, the type Terminal is registered to *run*.
    ///
    /// **The cost, said out loud.** ② is coarser than the type system it stands
    /// in for: a `readme.txt` somebody has `chmod +x`'d is `public.plain-text`
    /// to LaunchServices and would open in TextEdit, and this refuses it. That
    /// is the safe direction and the recoverable one — the reader is holding a
    /// window that will show them the file itself, and the refusal names the
    /// rule — where the other direction is a row in a files tree that ran
    /// something. Asking LaunchServices for the real type would mean the
    /// `UniformTypeIdentifiers` framework and a package this crate does not
    /// carry, for a sharper answer to a question whose wrong answers are not
    /// symmetric.
    ///
    /// **The disk is asked once, by the caller**, and this is the rule over
    /// what it said — for [`super::reveal_arguments`]' reason turned into a
    /// shape: the execute bit is not in the name, so a rule read off a string
    /// would be answering about a file nobody has looked at, and a rule that
    /// did its own `stat` would be a second one the call below could disagree
    /// with. The `metadata` here is the same one the URL's `isDirectory` is
    /// built from.
    ///
    /// **`path` is the *resolved* target and not the name a row carried**, and
    /// that is a repaired hole rather than a preference (RA-4, 2026-09-15).
    /// Clause ① reads a name and clause ② reads what the file system said, so
    /// when the two describe different files the gate answers about neither: a
    /// symbolic link called `notes` pointing at `Payload.app` is a directory
    /// (the target's `is_dir`) with no `.app` suffix (the link's name), and ①
    /// therefore did not fire on an application that `openURL:` — which follows
    /// the link — then launched. [`openable_target`] resolves before it asks, so
    /// the name and the mode here always belong to the same file, and that file
    /// is the one the URL is built from.
    fn opening_it_would_run_it(path: &Path, metadata: &std::fs::Metadata) -> bool {
        if metadata.is_dir() {
            return path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"));
        }
        metadata.permissions().mode() & 0o111 != 0
    }

    /// Hand one URL to LaunchServices, and say so when it will not take it.
    ///
    /// `openURL:` answers `NO` for a URL with no registered handler, a scheme
    /// this machine has nothing for, and a file that is not there. It does not
    /// say *which*, so neither does this: what the caller puts in front of a
    /// reader is the target it asked for and the fact the machine declined it,
    /// which is the same shape the Windows arm's `ShellExecuteW` failure has.
    fn hand_over(url: &NSURL, what: &str) -> Result<(), String> {
        let workspace = NSWorkspace::sharedWorkspace();
        if workspace.openURL(url) {
            Ok(())
        } else {
            Err(format!("the system has nothing registered to open {what}"))
        }
    }

    /// Ask the workspace to open one already-policy-checked address with its
    /// registered default handler.
    ///
    /// Which schemes may reach here deliberately belongs to the caller — in this
    /// product `webnav::address_bar` for a web address, and the reader's own
    /// `Ctrl`/`⌘`+click for a URI of any other scheme (owner ruling 2026-09-21) —
    /// and this bridge supplies the parse. `URLWithString:` is that parse, and its
    /// refusal is the honest one: a string that is not a URL never reaches
    /// LaunchServices, so there is nothing here for a target to be reparsed as.
    pub fn shell_execute(window: NativeWindow, target: &str) -> Result<(), String> {
        let _ = window;
        if target.contains('\0') {
            return Err("address contains an embedded NUL".to_owned());
        }
        let text = NSString::from_str(target);
        let url = NSURL::URLWithString(&text)
            .ok_or_else(|| format!("{target} is not an address this system can read"))?;
        hand_over(&url, target)
    }

    /// Open one worker-validated local image with its registered default
    /// handler.
    ///
    /// The caller must obtain `path` from a successful image decode record,
    /// never directly from terminal text; this bridge independently enforces
    /// the slice's syntax policy over the path it is handed, exactly as the
    /// Windows arm does, with this platform's spelling of "absolute".
    pub fn open_local_file(window: NativeWindow, path: &Path) -> Result<(), String> {
        let _ = window;
        openable_local_image(path)?;
        let url = file_url(path, false)?;
        hand_over(&url, &path.to_string_lossy())
    }

    /// **Decide what a row actually points at, and refuse it if opening it
    /// would run it** — the whole of [`open_local_path`]'s policy, with no call
    /// to LaunchServices in it, which is what lets the rule be tested on a
    /// machine that must not put a window on somebody's desk.
    ///
    /// The answer is the **resolved** path and whether that path is a
    /// directory, and the two gates are asked in this order for reasons that do
    /// not commute:
    ///
    /// 1. [`openable_unix_path`] first, **before any disk call**. It is lexical
    ///    — empty, NUL, relative — and a relative path must be refused for what
    ///    it is rather than resolved against this process's working directory,
    ///    which is the folder the shell that started Folio happened to be
    ///    standing in.
    /// 2. `canonicalize` next, which is the one question the rest is asked of.
    ///    It follows every link and settles every `..`, so it answers with the
    ///    file `openURL:` would reach — and a link pointing at nothing fails
    ///    here with the operating system's own words, in the same
    ///    `"{path:?}: {error}"` shape the plain missing file has always had, so
    ///    a reader is told the true fact (there is nothing there) rather than a
    ///    false one about programs.
    /// 3. [`opening_it_would_run_it`] last, asked of that resolved path and of
    ///    the metadata of that same path.
    ///
    /// [`PROGRAM_REFUSED`] is the sentence `bt-app` turns into *the tree does
    /// not run programs*, and every other refusal here is a fact about the
    /// machine that the same caller shows as a toast.
    fn openable_target(path: &Path) -> Result<(std::path::PathBuf, bool), String> {
        openable_unix_path(path)?;
        let real = std::fs::canonicalize(path).map_err(|error| format!("{path:?}: {error}"))?;
        let metadata = std::fs::metadata(&real).map_err(|error| format!("{real:?}: {error}"))?;
        if opening_it_would_run_it(&real, &metadata) {
            return Err(PROGRAM_REFUSED.to_owned());
        }
        Ok((real, metadata.is_dir()))
    }

    /// Open one file the user picked out of a directory listing with its
    /// registered default handler — and never run a program.
    ///
    /// The gates and their order are [`openable_target`]'s, and **the URL is
    /// built from the target that gate judged**, not from the name the row
    /// carried. Those have to be the same file: handing `openURL:` a link while
    /// having classified its target leaves the two able to disagree, which is
    /// exactly how a bundle once got past the refusal (RA-4).
    pub fn open_local_path(window: NativeWindow, path: &Path) -> Result<(), String> {
        let _ = window;
        let (real, directory) = openable_target(path)?;
        let url = file_url(&real, directory)?;
        hand_over(&url, &real.to_string_lossy())
    }

    /// **The same door, for a path a worker has already answered for** (owner ruling 2026-09-21)
    /// — `windows_handoff::open_local_path_verified`'s twin, and the arm where it actually buys
    /// something.
    ///
    /// [`openable_target`] canonicalises and stats, and `Ctrl`/`⌘`+click on a reference a program
    /// printed reaches it on the thread that paints: a target under a symlink into a mounted share
    /// stalls the window for as long as the mount takes to answer. All three facts that gate asks
    /// are the ledger's — the name is there, it is a folder or it is not, and opening it would run
    /// it — and the third is `bt_term::PathVerdict::executable`, answered off the same `metadata`
    /// the other two came from. The refusal is still this door's and still [`PROGRAM_REFUSED`].
    pub fn open_local_path_verified(
        window: NativeWindow,
        path: &Path,
        target: super::VerifiedTarget,
    ) -> Result<(), String> {
        let _ = window;
        openable_unix_path(path)?;
        if !target.exists {
            return Err(format!("{path:?}: not there"));
        }
        if target.executable {
            return Err(PROGRAM_REFUSED.to_owned());
        }
        // `openable_target`'s own answer: the URL is built from the target that gate judged, not
        // from the name the reference carried (RA-4). That target is [`resolved_for_a_door`]'s.
        let real = target
            .resolved
            .clone()
            .unwrap_or_else(|| path.to_path_buf());
        let url = file_url(&real, target.is_directory)?;
        hand_over(&url, &real.to_string_lossy())
    }

    /// Open Finder on a path, with the file **selected** inside its folder.
    ///
    /// `activateFileViewerSelectingURLs:` is `explorer /select,` without the
    /// command line: it takes an array of `NSURL`, Finder opens the containing
    /// folder for each and selects the item. A folder passed to it is selected
    /// in *its* parent, which is one level further out than a foot pointing at
    /// a root is offering — so a directory is opened instead, which is
    /// [`super::reveal_arguments`]' ruling and the same reading of it.
    ///
    /// **The disk is asked first, and that is the whole of this door's
    /// `Result`.** `activateFileViewerSelectingURLs:` returns nothing: it posts
    /// a request to Finder and a path that is not there produces no window and
    /// no answer. So the refusal has to be made before the call, which is
    /// exactly why the Windows arm canonicalises too — there, an argument that
    /// named nothing left Explorer opening a folder nobody asked for. Here
    /// `canonicalize` resolves the symlinks Finder would resolve, settles `..`,
    /// and fails in the operating system's own words when the path is gone.
    ///
    /// **No program is refused and none can be started.** A `.app` revealed is
    /// a `.app` sitting selected in a folder window, which is what somebody
    /// asking "where is this" wants to see — the same reasoning the Windows arm
    /// gives for having no extension gate on its reveal.
    pub fn reveal_in_explorer(window: NativeWindow, path: &Path) -> Result<(), String> {
        let _ = window;
        openable_unix_path(path)?;
        let real = std::fs::canonicalize(path).map_err(|error| format!("{path:?}: {error}"))?;
        let directory = std::fs::metadata(&real)
            .map_err(|error| format!("{real:?}: {error}"))?
            .is_dir();
        show(real, directory)
    }

    /// **The same reveal, for a path a worker has already answered for** (closure review of audit
    /// 3 C-2) — the twin of `windows_handoff::reveal_verified`, and for its reason.
    ///
    /// The door above asks the disk twice on the thread that paints, and it is reached from a
    /// `Ctrl`/`⌘`+click on a path a *program* printed: a target under a symlink into a mounted
    /// share stalls the window inside `canonicalize` for as long as the mount takes to answer. The
    /// two facts it wanted are the ledger's — the name is there, and it is a folder or it is not —
    /// so they arrive instead of being fetched. `NSURL` is built from the path as written, which
    /// is what Finder resolves anyway.
    pub fn reveal_verified(
        window: NativeWindow,
        path: &Path,
        target: super::VerifiedTarget,
    ) -> Result<(), String> {
        let _ = window;
        if !target.exists {
            return Err(format!("{path:?}: not there"));
        }
        openable_unix_path(path)?;
        show(
            target
                .resolved
                .clone()
                .unwrap_or_else(|| path.to_path_buf()),
            target.is_directory,
        )
    }

    /// The half both reveals end on: a folder is opened, a file is selected in its own.
    fn show(real: std::path::PathBuf, directory: bool) -> Result<(), String> {
        let url = file_url(&real, directory)?;
        let workspace = NSWorkspace::sharedWorkspace();
        if directory {
            // A folder is *opened*, not selected in its parent, and it is
            // opened **with Finder and without Finder taking the front**
            // (T-MAC-LIVE, §13.33 ④). See [`open_folder_in_finder`] for the
            // window that used to come up beside it.
            return open_folder_in_finder(url, &real.to_string_lossy());
        }
        workspace.activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
        Ok(())
    }

    /// **Open one folder in Finder and bring up that window, not the one Finder
    /// happened to be holding** (T-MAC-LIVE, §13.33 ④).
    ///
    /// `openURL:` on a directory does two things at once: it opens the folder's
    /// window, and it activates Finder — and activating an application brings
    /// its *key* window forward along with it. Measured on macOS 26.6.2 with
    /// nine Finder windows on the desk and a reference window of the ticket's
    /// own in front of all of them: `openURL:` put **two** Finder windows above
    /// that reference, reproducibly — the folder the reader asked for, and
    /// whichever window Finder had been holding before. A reader who asks a
    /// terminal "show me this folder" did not ask for the other one.
    ///
    /// So the two halves are separated. `NSWorkspaceOpenConfiguration` with
    /// `activates = false` opens the window and leaves the front where it was;
    /// `NSRunningApplication::activate` with **no options** then brings Finder
    /// forward, and Apple documents the default option set as main-and-key only
    /// — `NSApplicationActivateAllWindows` is the flag that would do what the
    /// single call was doing. The same measurement then reads **one**.
    ///
    /// **The `Result` is still answered before the call, which is this module's
    /// habit and not a new one** (see [`reveal_in_explorer`] on why the disk is
    /// asked first). The configuration form answers in a block, and a door that
    /// returned `Ok` and then discovered otherwise would be lying to a caller
    /// that has already drawn a foot; so the question asked here is the one this
    /// call actually depends on — *is there a Finder on this machine* —
    /// answered synchronously by `URLForApplicationWithBundleIdentifier:`,
    /// exactly as [`open_system_fonts_page`] asks it about Font Book. The
    /// completion handler is then genuinely nothing this door needs, and the
    /// activation rides in it because that is where the running application is
    /// handed over; `NSRunningApplication` is documented thread-safe, which is
    /// the same paragraph of Apple's *Thread Safety Summary* §13.18 quotes for
    /// `NSWorkspace`.
    fn open_folder_in_finder(url: Retained<NSURL>, what: &str) -> Result<(), String> {
        let workspace = NSWorkspace::sharedWorkspace();
        let finder = workspace
            .URLForApplicationWithBundleIdentifier(ns_string!("com.apple.finder"))
            .ok_or_else(|| format!("this machine has no Finder to open {what} in"))?;
        let configuration = NSWorkspaceOpenConfiguration::configuration();
        configuration.setActivates(false);
        let handler = RcBlock::new(
            move |running: *mut NSRunningApplication, _error: *mut NSError| {
                let _callback = crate::admission::enter_callback("finder-open");
                // The default option set, which is the whole ruling: main and
                // key, never `NSApplicationActivateAllWindows`.
                // SAFETY: the block is AppKit's to call and the pointer is
                // AppKit's to hand over — non-null when the open succeeded,
                // null when it did not, which is the whole of what is read here.
                if let Some(running) = unsafe { running.as_ref() } {
                    running.activateWithOptions(NSApplicationActivationOptions::empty());
                }
            },
        );
        workspace.openURLs_withApplicationAtURL_configuration_completionHandler(
            &NSArray::from_retained_slice(&[url]),
            &finder,
            &configuration,
            Some(&handler),
        );
        Ok(())
    }

    /// **Open the system's font page** (user ruling 2026-08-19), which on macOS
    /// is Font Book.
    ///
    /// The Windows arm hands `ShellExecuteW` a `ms-settings:` URI and falls
    /// back to `%WINDIR%\Fonts` through Explorer. Neither half crosses: this
    /// platform has no settings URI for fonts, and the folders a font lives in
    /// (`/System/Library/Fonts`, `/Library/Fonts`, `~/Library/Fonts`) are three
    /// places rather than one, so opening "the fonts folder" would be choosing
    /// one of them on the reader's behalf. Font Book is the single answer, and
    /// it is what a Mac installs a font with.
    ///
    /// **LaunchServices is asked where it is, rather than told.**
    /// `/System/Applications/Font Book.app` is where it stands today and a path
    /// spelled here would be a claim that goes stale on an OS update — the
    /// mistake `bt-render`'s macOS font loader documents at length about the
    /// `AssetsV2` font assets. `URLForApplicationWithBundleIdentifier:` asks the
    /// database that actually knows, and its `nil` is the honest refusal for a
    /// machine whose Font Book has been removed.
    ///
    /// **This product installs and deletes nothing**, which is the Windows
    /// arm's closing paragraph and is not a platform fact: a font is a
    /// machine-wide resource and the door is the whole feature.
    pub fn open_system_fonts_page(window: NativeWindow) -> Result<(), String> {
        let _ = window;
        let workspace = NSWorkspace::sharedWorkspace();
        let url = workspace
            .URLForApplicationWithBundleIdentifier(ns_string!("com.apple.FontBook"))
            .ok_or_else(|| "this machine has no Font Book".to_owned())?;
        hand_over(&url, "the system font settings")
    }

    /// **These run on a Mac and only on a Mac**, which is the point of them:
    /// every rule above is a claim about what LaunchServices and Finder do, and
    /// a claim about another program's behaviour cannot be checked by a source
    /// pin. The Windows side of this ticket holds the *signatures* instead
    /// (`macos_process_door_tests`).
    ///
    /// **Two of them make something visible happen on the desk**, and that is
    /// deliberate rather than careless — a door that reports success while
    /// nothing opened is precisely the failure they exist to catch. Each says
    /// in its own note what it leaves behind and what it takes back.
    #[cfg(test)]
    mod tests {
        use std::os::unix::fs::symlink;
        use std::path::PathBuf;

        use objc2::rc::Retained;
        use objc2_app_kit::NSRunningApplication;
        use objc2_foundation::NSBundle;

        use super::*;

        /// **Whether this run may put something on the desk.** The two cases
        /// below open a Finder window and launch an editor; on a machine where
        /// a person is working, an unasked-for window and the panel a vanished
        /// folder raises are an intrusion, so they run only under the consent
        /// every window-opening target of this crate reads — `BT_MAC_GUI=1`
        /// (`docs/BT-ENVIRONMENT.md`). Without it each prints one line and
        /// passes, and the door's answers are still held by the refusal cases.
        fn desk_consent() -> bool {
            if std::env::var_os("BT_MAC_GUI").is_some() {
                return true;
            }
            eprintln!("skipped — set BT_MAC_GUI to let this case open a window on the desk");
            false
        }

        /// A directory of this test's own, under the system's temporary
        /// directory and named for this process, removed however the case ends.
        fn scratch(name: &str) -> PathBuf {
            let directory =
                std::env::temp_dir().join(format!("folio-handoff-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&directory).expect("a scratch directory");
            directory
        }

        /// RED — **the tree does not run programs, and on this platform that is
        /// a fact about the file rather than about its name.**
        ///
        /// The two shapes where opening is running, and the one where it is
        /// not, each asked of a real file on a real disk: a `.app` directory, a
        /// file with the execute bit, and an ordinary document.
        ///
        /// MUTATION: drop the directory clause and a bundle in a files column
        /// becomes a row that launches an application; drop the mode clause and
        /// a shell script becomes a row that runs it in Terminal.
        #[test]
        fn opening_a_bundle_or_an_executable_would_run_it_and_a_document_would_not() {
            let root = scratch("programs");
            let bundle = root.join("Thing.app");
            std::fs::create_dir_all(&bundle).expect("a bundle");
            let folder = root.join("plain-folder");
            std::fs::create_dir_all(&folder).expect("a folder");
            let script = root.join("run-me");
            std::fs::write(&script, b"#!/bin/sh\nexit 0\n").expect("a script");
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("the execute bit");
            let document = root.join("notes.txt");
            std::fs::write(&document, b"x").expect("a document");

            let says = |path: &std::path::Path| {
                let metadata = std::fs::metadata(path).expect("a file that is there");
                opening_it_would_run_it(path, &metadata)
            };
            assert!(says(&bundle), "a `.app` is what an application is here");
            assert!(says(&script), "the execute bit is the type Terminal runs");
            assert!(!says(&folder), "an ordinary folder opens in Finder");
            assert!(!says(&document), "a document opens in its editor");

            // And the door itself says the product's sentence for the first two,
            // which is the one `bt-app` matches on.
            let window = crate::NativeWindow::stand_in(0);
            for program in [&bundle, &script] {
                assert_eq!(
                    open_local_path(window, program),
                    Err(PROGRAM_REFUSED.to_owned()),
                    "{program:?} is a program and this door does not run one"
                );
            }
            let _ = std::fs::remove_dir_all(&root);
        }

        /// RED — **the refusal is about the file that will actually be
        /// opened**, so a symbolic link is judged by its target and handed on
        /// as its target (RA-4, 2026-09-15).
        ///
        /// The finding's own shape is ① below: a link *named* `notes` pointing
        /// at `Payload.app`. Before the fix the gate asked `is_dir` of the
        /// target and `extension` of the link, so that file was a directory
        /// with no `.app` suffix and the bundle clause did not fire — while
        /// `openURL:`, which follows links, launched the application. The
        /// execute-bit clause never had the hole, because a mode can only be
        /// read off the resolved file; ② keeps it that way.
        ///
        /// The other half of the ruling is that resolving must not turn every
        /// link into a refusal (③, ④) and must not turn a link to nothing into
        /// a claim about programs (⑥) — the reader is owed the true fact.
        ///
        /// **Nothing opens.** Every case here is refused, or stops at the gate
        /// [`openable_target`] before LaunchServices is reached.
        ///
        /// MUTATION: resolve for the metadata but classify the path as given
        /// and ① goes green-to-red; build the URL from the path as given
        /// instead of from the resolved target and ③'s second half fails.
        #[test]
        fn a_link_is_judged_by_what_it_points_at_and_opened_as_that() {
            let root = scratch("links");
            let bundle = root.join("Payload.app");
            std::fs::create_dir_all(&bundle).expect("a bundle");
            let folder = root.join("plain-folder");
            std::fs::create_dir_all(&folder).expect("a folder");
            let script = root.join("run-me");
            std::fs::write(&script, b"#!/bin/sh\nexit 0\n").expect("a script");
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("the execute bit");
            let document = root.join("notes.txt");
            std::fs::write(&document, b"x").expect("a document");

            let link = |target: &std::path::Path, name: &str| {
                let at = root.join(name);
                symlink(target, &at).expect("a symbolic link");
                at
            };
            let to_bundle = link(&bundle, "notes");
            let to_script = link(&script, "harmless.txt");
            let to_document = link(&document, "readme");
            let to_folder = link(&folder, "elsewhere");
            let missing = root.join("never-was");
            let dangling = link(&missing, "gone");

            let refused = Err(PROGRAM_REFUSED.to_owned());
            let window = crate::NativeWindow::stand_in(0);

            // ① The finding: a directory target with no `.app` in the *link's*
            //    name. Both the gate and the door say the product's sentence.
            assert_eq!(
                openable_target(&to_bundle),
                refused,
                "a link pointing at an application is an application"
            );
            assert_eq!(
                open_local_path(window, &to_bundle),
                Err(PROGRAM_REFUSED.to_owned())
            );
            // ② An executable behind a link, and the `.txt` on the link's name
            //    is the lie the mode sees through.
            assert_eq!(
                openable_target(&to_script),
                refused,
                "a link pointing at a program is a program"
            );
            // ③ A link to an ordinary document still opens, and what is opened
            //    is the resolved target — the file the gate judged.
            assert_eq!(
                openable_target(&to_document),
                Ok((
                    std::fs::canonicalize(&document).expect("the document resolves"),
                    false
                )),
                "resolving may not turn every link into a refusal"
            );
            // ④ The `isDirectory` the URL is built with is the resolved
            //    target's too, so a link to a folder opens as a folder.
            assert_eq!(
                openable_target(&to_folder),
                Ok((
                    std::fs::canonicalize(&folder).expect("the folder resolves"),
                    true
                ))
            );
            // ⑤ Named directly, nothing has changed.
            assert_eq!(openable_target(&bundle), refused);
            assert_eq!(
                openable_target(&script),
                refused,
                "the execute bit never had the hole"
            );
            assert_eq!(
                openable_target(&document),
                Ok((
                    std::fs::canonicalize(&document).expect("the document resolves"),
                    false
                ))
            );
            // ⑥ A link to nothing is refused in the words the operating system
            //    uses for a file that is not there — the same refusal a plain
            //    missing path has always had, and never the product's sentence.
            let words = |reason: &str, path: &std::path::Path| {
                reason
                    .strip_prefix(&format!("{path:?}: "))
                    .map(str::to_owned)
            };
            let broken = openable_target(&dangling).expect_err("a link to nothing opens nothing");
            let gone = openable_target(&missing).expect_err("a path that is not there");
            assert!(
                !broken.contains(PROGRAM_REFUSED),
                "a link to nothing is not a program: {broken:?}"
            );
            assert!(
                words(&broken, &dangling).is_some(),
                "the refusal names the path the reader gave: {broken:?}"
            );
            assert_eq!(
                words(&broken, &dangling),
                words(&gone, &missing),
                "a link to nothing is refused in the same words a missing file is"
            );

            let _ = std::fs::remove_dir_all(&root);
        }

        /// RED — **every refusal this door can give is a sentence, and none of
        /// them is the product's own.**
        ///
        /// A relative path, a path that is not there, a picture lane handed
        /// something that is not a picture, a reveal of nothing, and an address
        /// carrying a NUL: five different mistakes, five reasons a toast can
        /// carry, and not one of them
        /// [`PROGRAM_REFUSED`] — which `bt-app` matches with `.contains` and
        /// turns into *the tree does not run programs*. A refusal that
        /// accidentally carried that sentence would tell a reader their file is
        /// an executable because the disk was busy.
        ///
        /// **Nothing opens.** Each of these is the real call, and each is
        /// refused before LaunchServices is reached.
        #[test]
        fn a_refusal_carries_its_own_reason_and_never_the_products() {
            let window = crate::NativeWindow::stand_in(0);
            let missing = scratch("refusals").join("not-here.txt");
            let refusals = [
                open_local_path(window, std::path::Path::new("notes/a.txt")),
                open_local_path(window, &missing),
                open_local_file(window, std::path::Path::new("/tmp/a.txt")),
                reveal_in_explorer(window, &missing),
                shell_execute(window, "https://example.invalid/\0"),
            ];
            for refusal in refusals {
                let reason = refusal.expect_err("each of these is refused");
                assert!(!reason.is_empty(), "a refusal says something");
                assert!(
                    !reason.contains(PROGRAM_REFUSED),
                    "a refusal about the machine wears the product's sentence: {reason:?}"
                );
            }
            let _ = std::fs::remove_dir_all(scratch("refusals"));
        }

        /// RED — **a reveal asks the disk before it asks Finder**, because
        /// `activateFileViewerSelectingURLs:` has no answer of its own.
        ///
        /// The `Result` this door gives is made entirely of what the file
        /// system said: canonicalise, `stat`, and only then post the request.
        /// So the case worth pinning is the pair — a path that is gone is an
        /// `Err` in the operating system's own words, and a path that is there
        /// is `Ok`.
        ///
        /// **This opens one Finder window**, on a directory under the system's
        /// temporary folder, and leaves it open: closing somebody else's window
        /// needs an Apple event this process is not authorized to send, and the
        /// venue's rules forbid reaching for another program by name. One
        /// window is the honest price of proving the door reaches Finder at
        /// all.
        #[test]
        fn a_reveal_asks_the_disk_before_it_asks_finder() {
            if !desk_consent() {
                return;
            }
            let window = crate::NativeWindow::stand_in(0);
            let root = scratch("reveal");
            let file = root.join("a file, with a \" in it.txt");
            std::fs::write(&file, b"x").expect("a file to point at");

            assert!(
                reveal_in_explorer(window, &root.join("gone.txt")).is_err(),
                "a path that is not there is refused rather than shown"
            );
            assert_eq!(
                reveal_in_explorer(window, &file),
                Ok(()),
                "a quote in a name is an ordinary character here — there is no \
                 command line for it to break"
            );
            // The folder half takes the other road (`openURL:`), and that one
            // does answer.
            assert_eq!(reveal_in_explorer(window, &root), Ok(()));
            // The directory Finder was just pointed at stays where it is: a
            // Finder window whose folder is deleted under it puts a "cannot
            // be found" panel on the desk (measured on the owner's screen,
            // 2026-09-13), and a few bytes under the temporary directory are
            // cheaper than that.
        }

        /// RED — **a file handed to the workspace really opens**, which is the
        /// one claim about LaunchServices that nothing else in this file can
        /// make.
        ///
        /// # It ends what it started, and it cannot end anything else
        ///
        /// Two conditions have to hold together before this asks anything to
        /// quit, and either alone would be a test that can reach somebody
        /// else's work:
        ///
        /// ① **it was not running before.** The applications running before the
        ///    call are written down, and the candidate must not be among them.
        ///    Apple's note on `processIdentifier` says to compare processes with
        ///    `isEqual:` rather than with a pid, and that is what the search
        ///    uses — so a TextEdit the reader already had open is in the
        ///    *before* list and can never match.
        /// ② **it is the application LaunchServices named for this file.**
        ///    `URLForApplicationToOpenURL:` is asked *before* the door is
        ///    called, and the candidate's `bundleURL` has to be that one. ①
        ///    alone is not enough: anything at all may launch during the second
        ///    this test waits — a reader double-clicking something, an agent
        ///    somebody else is running, a helper the system starts — and
        ///    "whatever appeared next" is not a description of what this test
        ///    opened.
        ///
        /// Nothing is asserted about the quit. The claim is the door's answer;
        /// ending the application is tidiness, and a handler that was already
        /// running reused its process, so there is nothing to end. The
        /// terminate is asked once the application says it has finished
        /// launching, because one asked mid-launch is documented to be refused.
        #[test]
        fn open_local_path_hands_a_file_to_the_workspace() {
            if !desk_consent() {
                return;
            }
            let window = crate::NativeWindow::stand_in(0);
            let root = scratch("open");
            let file = root.join("folio-m2-2.txt");
            std::fs::write(&file, b"M2-2 opened this.\n").expect("a document");

            // **Which application, asked before the door is called.**
            // `URLForApplicationToOpenURL:` is LaunchServices' own answer to
            // "who opens this", and its bundle identifier is the only thing
            // this case will ever act on.
            let workspace = NSWorkspace::sharedWorkspace();
            let url = file_url(&file, false).expect("a file URL");
            let handler = workspace
                .URLForApplicationToOpenURL(&url)
                .and_then(|bundle| NSBundle::bundleWithURL(&bundle))
                .and_then(|bundle| bundle.bundleIdentifier());
            // **Every instance of it that is already running.** Asked through
            // the same fresh query used afterwards, so the two lists are the
            // same kind of answer and the difference between them is real.
            let standing = |identifier: &NSString| -> Vec<Retained<NSRunningApplication>> {
                NSRunningApplication::runningApplicationsWithBundleIdentifier(identifier).to_vec()
            };
            let before = handler.as_deref().map(standing).unwrap_or_default();

            assert_eq!(
                open_local_path(window, &file),
                Ok(()),
                "the workspace took the document"
            );

            let Some(handler) = handler else {
                // No registered handler to name, so there is nothing this case
                // may end. The claim above stands either way.
                let _ = std::fs::remove_dir_all(&root);
                return;
            };
            // **The fresh query, not `-[NSWorkspace runningApplications]`, and
            // that is the finding this case cost.** The workspace's array is a
            // *cache* kept current by `NSWorkspaceDidLaunchApplicationNotification`,
            // and Apple documents it as updating when the **main** run loop is
            // spun. A `#[test]` body runs on a thread the harness made, so a
            // version of this loop built on that array watched the same
            // sixty-nine applications for ten seconds while the seventieth —
            // the one it had just launched — stood on the desk unseen, and the
            // TextEdit it meant to end stayed open.
            // `+[NSRunningApplication runningApplicationsWithBundleIdentifier:]`
            // asks LaunchServices instead of reading a cache, which is an answer
            // any thread can have. It is the rule for every later macOS case
            // that waits on another application's state.
            let mut ended: Option<Retained<NSRunningApplication>> = None;
            for _ in 0..100 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let opened = standing(&handler)
                    .into_iter()
                    .find(|running| !before.iter().any(|known| **known == **running));
                if let Some(opened) = opened
                    && opened.isFinishedLaunching()
                {
                    opened.terminate();
                    ended = Some(opened);
                    break;
                }
            }

            // **The document outlives the quit, and that ordering is the whole
            // of it.** An earlier version removed the directory as soon as it
            // had asked, and the application it had just handed a file to then
            // found that file gone — which is a *modal* question on this
            // platform, and a modal question is one no quit request gets past.
            // So the wait is here, before the file goes.
            //
            // **And "is it gone" is asked the same way "is it there" was.**
            // `isTerminated` is KVO-backed and reads the same cache the
            // workspace's array does, so on this thread it answers `false` for
            // a process that has already quit; the honest question is whether
            // LaunchServices still lists the instance.
            let mut gone = ended.is_none();
            if let Some(ended) = &ended {
                for _ in 0..50 {
                    if !standing(&handler)
                        .iter()
                        .any(|running| **running == **ended)
                    {
                        gone = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            // For a reader running `-- --nocapture`. Neither half is asserted —
            // the claim is the door's answer — but a case that can leave an
            // application standing on somebody's desk should say so rather than
            // let them find it.
            println!(
                "handed the document to {handler}; {} were running before; {}",
                before.len(),
                match (&ended, gone) {
                    (None, _) => "nothing new appeared to end".to_owned(),
                    (Some(_), true) => "the one this opened has gone again".to_owned(),
                    (Some(_), false) =>
                        "the one this opened was asked to quit and is still standing".to_owned(),
                }
            );
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}

/// The Windows half: the real directories, the real `PATHEXT`, the real disk.
#[cfg(windows)]
pub use windows_handoff::{
    open_local_file, open_local_path, open_local_path_verified, open_system_fonts_page,
    program_on_path, reveal_in_explorer, reveal_verified, shell_execute,
};

#[cfg(windows)]
mod windows_handoff {
    #[cfg(not(test))]
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use crate::NativeWindow;
    use windows::Win32::Foundation::MAX_PATH;
    use windows::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
    #[cfg(not(test))]
    use windows::Win32::UI::Shell::ShellExecuteW;
    #[cfg(not(test))]
    use windows::Win32::UI::WindowsAndMessaging::{
        ASFW_ANY, AllowSetForegroundWindow, SW_SHOWNORMAL,
    };
    #[cfg(not(test))]
    use windows::core::PCWSTR;

    use super::{
        DEFAULT_PATHEXT, PROGRAM_REFUSED, VerifiedTarget, names_a_program, normalised_target,
        program_in_directories, reveal_argument_form, reveal_arguments, validate_local_image_path,
        validate_openable_path,
    };
    // `the_system_calls_it_dangerous` stood here from audit 3 C-4 (2026-09-20) until the closure
    // re-review of 2026-09-21. It put `AssocIsDangerous` and `SHGetFileInfo(SHGFI_EXETYPE)` in
    // front of `ShellExecuteW` as a second floor under the extension list.
    //
    // It is gone because the door it guarded has no attacker-driven caller left. C-4's real fix is
    // provenance: a reference a *program* printed is revealed and never opened, which `bt-app`'s
    // `nothing_a_program_printed_reaches_the_shells_open_verb` holds structurally. What remains
    // here are three surfaces where the **user** picked the file by name -- `Open with...`, the
    // breadcrumb's `Open` menu and the no-preview card's button -- and on those the shell's own
    // unsafe-association list refused macro-bearing Office documents with the sentence "the files
    // tree does not run programs", which is simply false about a `.docm`: the handler is Word, and
    // Word guards its own macros. A floor that refuses a document the reader named by hand is
    // worse than the list it was added to.

    /// **Enter a single-threaded apartment with OLE1 DDE off** — Microsoft's
    /// instruction for any thread that calls `ShellExecuteW` (see
    /// [`super::ShellThread`]). Answers whether this call entered one and so owes
    /// [`leave_apartment`]; a thread already in an apartment of another kind
    /// answers `RPC_E_CHANGED_MODE`, owes nothing, and hands off as it is.
    pub(super) fn enter_apartment() -> bool {
        use windows::Win32::System::Com::{
            COINIT, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
        };
        // SAFETY: no reserved pointer, a documented flag pair, on the thread that
        // will make every hand-off; balanced by `leave_apartment` on the same
        // thread when `ShellThread` drops.
        let entered = unsafe {
            CoInitializeEx(
                None,
                COINIT(COINIT_APARTMENTTHREADED.0 | COINIT_DISABLE_OLE1DDE.0),
            )
        };
        entered.is_ok()
    }

    /// The matching leave, on the thread that entered.
    pub(super) fn leave_apartment() {
        // SAFETY: called only when `enter_apartment` answered `true`, on the
        // same thread (`ShellThread` is not `Send`).
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }

    /// **The one `ShellExecuteW` in this workspace.**
    ///
    /// Every hand-off above it has already decided *what* it is handing over;
    /// this decides how, and the how is the same every time: an explicit
    /// operation, an explicit working directory that is never the one this
    /// process happens to be standing in, and NUL-terminated UTF-16 that stays
    /// alive across the synchronous call.
    ///
    /// **The directory is never null** (R1-17). A null `lpDirectory` means "use
    /// the process's current directory", and the process's current directory is
    /// whatever folder the shell that started Folio was in — which becomes the
    /// working directory of the browser, editor or Explorer window that opens,
    /// and therefore the first place that program looks for the libraries it
    /// loads. Each caller names a directory it can defend: a file's own folder
    /// for a file, the system directory for an address that has no folder.
    ///
    /// **It runs on the OS hand-off lane, never on the window thread**
    /// (`docs/DESIGN.md`, 2026-09-22 — *a hand-off to the system runs on its own
    /// lane*). Two things follow from that, and both are written here because
    /// this is the one place either could be undone:
    ///
    /// * **No owner window.** `ShellExecuteW`'s `hwnd` is the owner of whatever
    ///   UI the call raises — an error box, the "how do you want to open this"
    ///   picker, a shell extension's own dialog. A window owned across threads
    ///   attaches the two threads' input queues, so the window thread would
    ///   share its input with a lane that is, by design, allowed to sit inside a
    ///   shell extension for a second and a half. `None` makes that UI a
    ///   top-level window of this process, which is the foreground process at
    ///   the moment of the press, so it still comes up in front. `window` is the
    ///   window that asked; it stays in the signature because a door here has one
    ///   signature on every platform.
    /// * **The receiver may take the front** —
    ///   [`let_the_receiver_take_the_front`], immediately before the call. See it
    ///   for why the grant is not scoped to one process.
    fn hand_over(
        window: NativeWindow,
        program: &str,
        arguments: Option<&std::ffi::OsStr>,
        directory: &Path,
    ) -> Result<(), String> {
        let _ = window;
        let_the_receiver_take_the_front();
        shell_execute_w(program, arguments, directory)
    }

    /// **The call itself** — `ShellExecuteW` with no owner, the `open` verb and
    /// the directory the caller defended.
    #[cfg(not(test))]
    fn shell_execute_w(
        program: &str,
        arguments: Option<&std::ffi::OsStr>,
        directory: &Path,
    ) -> Result<(), String> {
        let mut operation = wide("open");
        let mut program = wide(program);
        let mut arguments = arguments.map(wide_os);
        let mut directory = wide_os(directory.as_os_str());
        // SAFETY: every buffer below is a live, NUL-terminated UTF-16 buffer
        // for the duration of this synchronous call, the caller's gate has
        // refused any embedded NUL, and no window is named.
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(operation.as_mut_ptr()),
                PCWSTR(program.as_mut_ptr()),
                arguments
                    .as_mut()
                    .map_or(PCWSTR::null(), |arguments| PCWSTR(arguments.as_mut_ptr())),
                PCWSTR(directory.as_mut_ptr()),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// **The same call in this crate's own tests: written down, never made.**
    ///
    /// The seam `the_reveal_grants_the_foreground_before_it_hands_over` reads.
    /// What it can hold is the *order* of the two calls and the exact program
    /// and argument handed over — the real producer's output — and what it
    /// cannot hold is what Explorer then does with them, which is the owner's
    /// machine's question and not a unit test's.
    #[cfg(test)]
    fn shell_execute_w(
        program: &str,
        arguments: Option<&std::ffi::OsStr>,
        directory: &Path,
    ) -> Result<(), String> {
        recorded::note(format!(
            "ShellExecuteW {program} {} in {}",
            arguments.map_or_else(String::new, |arguments| arguments
                .to_string_lossy()
                .into_owned()),
            directory.display()
        ));
        Ok(())
    }

    /// **Let whichever process receives this hand-off come to the front**
    /// (owner, 2026-09-22: 「接手的窗口必须在最前」).
    ///
    /// Windows gives the foreground only to a process the foreground process
    /// has said may take it. The process `ShellExecuteW` starts is covered by
    /// that rule on its own — it was started by the foreground process — but
    /// **the process that ends up showing the window is often not the one that
    /// was started**: `explorer.exe /select,…` forwards the request to the
    /// Explorer that is already running and exits, and a single-instance editor
    /// forwards the file to its running copy the same way. The window then
    /// opens, or an existing one is reused, behind Folio — the owner's report
    /// of 2026-09-22. A grant scoped to one process id would name the process
    /// that was started and exits, which is the wrong one, and the process that
    /// will receive the forward is not knowable from here.
    ///
    /// So the grant is `ASFW_ANY`, made **by this process, in answer to the
    /// press the reader just made, immediately before the hand-off that press
    /// asked for.** Review C-7 refused `ASFW_ANY` at the launch pipe for a
    /// reason that does not apply here: there the process id came off a wire, so
    /// a peer that had taken the pipe's name could spend this process's
    /// foreground on whatever it liked, at a moment of its choosing. Nothing
    /// reaches this call from outside — it takes no argument at all — and the
    /// grant lapses at the reader's next input or the moment any process takes
    /// the foreground, which the receiver does straight away.
    /// `hotkey::allow_foreground_for` keeps its refusal of `u32::MAX`, because
    /// that door is still the one a process id from outside reaches.
    ///
    /// Failure is not reported: it answers `false` when this process is no
    /// longer the one allowed to grant (the reader clicked elsewhere first), and
    /// the worst it costs is the window opening behind, which is today's
    /// behaviour.
    fn let_the_receiver_take_the_front() {
        #[cfg(test)]
        recorded::note("AllowSetForegroundWindow(ASFW_ANY)".to_owned());
        // SAFETY: a call taking one integer and dereferencing nothing.
        #[cfg(not(test))]
        let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    }

    #[cfg(not(test))]
    fn wide(text: &str) -> Vec<u16> {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        units.push(0);
        units
    }

    #[cfg(not(test))]
    fn wide_os(text: &std::ffi::OsStr) -> Vec<u16> {
        let mut units: Vec<u16> = text.encode_wide().collect();
        units.push(0);
        units
    }

    /// The folder a launch for `path` runs in: the file's own, or the path
    /// itself when it is a folder or a drive root.
    fn folder_of(path: &Path) -> PathBuf {
        match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => path.to_path_buf(),
        }
    }

    /// `C:\Windows\System32` as Windows itself answers it — the working
    /// directory for a hand-off that names no folder of its own.
    fn system_directory() -> PathBuf {
        read_directory(&|buffer| unsafe { GetSystemDirectoryW(Some(buffer)) })
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32"))
    }

    fn windows_directory() -> PathBuf {
        read_directory(&|buffer| unsafe { GetWindowsDirectoryW(Some(buffer)) })
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
    }

    /// The two directory calls share one reading: they answer the length they
    /// wrote, and zero is the failure.
    fn read_directory(call: &dyn Fn(&mut [u16]) -> u32) -> Option<PathBuf> {
        let mut buffer = [0u16; MAX_PATH as usize + 1];
        let written = call(&mut buffer) as usize;
        if written == 0 || written > buffer.len() {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&buffer[..written])))
    }

    /// **The absolute program a name means** — [`program_in_directories`] over
    /// this machine's own directories, and never the working directory.
    ///
    /// `None` when nothing by that name is anywhere a program is supposed to
    /// live, and `None` is a refusal rather than an invitation to try the bare
    /// name: a bare name is exactly what would fall back to the folder the
    /// reader is standing in.
    #[must_use]
    pub fn program_on_path(name: &Path) -> Option<PathBuf> {
        let mut directories = vec![system_directory(), windows_directory()];
        if let Some(path) = std::env::var_os("PATH") {
            // Absolute entries only: an empty entry, and a relative one, both
            // mean the working directory to Windows.
            directories.extend(
                std::env::split_paths(&path)
                    .filter(|entry| !entry.as_os_str().is_empty() && entry.is_absolute()),
            );
        }
        let pathext = std::env::var("PATHEXT").unwrap_or_default();
        let pathext = if pathext.trim().is_empty() {
            DEFAULT_PATHEXT.to_owned()
        } else {
            pathext
        };
        program_in_directories(name, &directories, &pathext, &|candidate| {
            candidate.is_file()
        })
    }

    /// Ask Windows to open one already-policy-checked address with its
    /// registered default handler.
    ///
    /// Which schemes may reach here deliberately belongs to the caller — in this
    /// product `webnav::address_bar` for a web address, and the reader's own
    /// `Ctrl`+click for a URI of any other scheme (owner ruling 2026-09-21) — and
    /// this bridge supplies the audited UTF-16 boundary and the working
    /// directory. No parameters are supplied, so the address is never reparsed
    /// as a command line.
    pub fn shell_execute(window: NativeWindow, target: &str) -> Result<(), String> {
        if target.contains('\0') {
            return Err("ShellExecuteW target contains an embedded NUL".to_owned());
        }
        hand_over(window, target, None, &system_directory())
    }

    /// Open one worker-validated local image with its registered default
    /// handler.
    ///
    /// The caller must obtain `path` from a successful image decode record,
    /// never directly from terminal text. This bridge independently enforces
    /// the slice's immutable syntax policy (drive-rooted, supported extension,
    /// no embedded NUL, no verbatim spelling) over the name **Windows will
    /// resolve**, and supplies no parameters, preventing command-line
    /// reinterpretation. It performs no event-thread file I/O.
    pub fn open_local_file(window: NativeWindow, path: &Path) -> Result<(), String> {
        let path = normalised_target(path).ok_or_else(|| "path has no name".to_owned())?;
        validate_local_image_path(&path)?;
        hand_over(window, &path.to_string_lossy(), None, &folder_of(&path))
    }

    /// Open one file the user picked out of a directory listing with its
    /// registered default handler — and never run a program.
    ///
    /// **Why a second bridge rather than widening the first.**
    /// [`open_local_file`] serves paths *scraped out of terminal text*, where
    /// the only defence against a hostile line of output is that the syntax
    /// policy is narrow enough to be immutable. A row of the files tree has the
    /// opposite provenance: the user chose the root, this process enumerated
    /// the directory, and the user pressed the row. Making the two share one
    /// validator would mean either the tree can open nothing but pictures or
    /// terminal output can open anything.
    ///
    /// **Why programs are refused.** Not as a hedge — as the product rule
    /// `DESIGN.md` §7.1.3 already implies by making activation mean *open the
    /// preview*: the tree is a way of looking at files, and the thing next to
    /// it that runs programs is the terminal, where running one is a line you
    /// typed and can see.
    ///
    /// **The refusal and the call read one name** (R1-11). The path is
    /// normalised first and everything below — the extension check and the
    /// hand-off — uses that one value, so there is no spelling in which the two
    /// can be talking about different files.
    pub fn open_local_path(window: NativeWindow, path: &Path) -> Result<(), String> {
        let path = normalised_target(path).ok_or_else(|| "path has no name".to_owned())?;
        validate_openable_path(&path)?;
        // **The list, and only the list** (closure re-review, 2026-09-21). The three surfaces
        // that reach here are ones where the user picked the file by name, and the terminal's own
        // references cannot reach this door at all — see the note above for the floor that was
        // tried here and taken out again.
        if names_a_program(&path, std::env::var("PATHEXT").unwrap_or_default().as_str()) {
            return Err(PROGRAM_REFUSED.to_owned());
        }
        hand_over(window, &path.to_string_lossy(), None, &folder_of(&path))
    }

    /// **The same door, for a path a worker has already answered for** (owner ruling 2026-09-21).
    ///
    /// `Ctrl`+click on a reference a program printed opens it with the machine's registered
    /// handler, exactly as it always has — but the two questions that decide *whether* it may are
    /// now the ledger's rather than this thread's. On Windows the door above asks no disk at all,
    /// so the only thing this adds is the existence check the caller used to get for free from the
    /// shell's own failure: a name that is not there must not reach `ShellExecuteW`, which answers
    /// a bare error code the user never sees.
    ///
    /// `names_a_program` is asked by that door, unchanged and with the list it has always had — the
    /// refusal is the door's, not the caller's, which is `docs/DESIGN.md`'s
    /// 「拒绝写在门上而不是写在每个敲门的人身上」.
    pub fn open_local_path_verified(
        window: NativeWindow,
        path: &Path,
        target: VerifiedTarget,
    ) -> Result<(), String> {
        if !target.exists {
            return Err(format!("{path:?}: not there"));
        }
        open_local_path(window, path)
    }

    /// Open Explorer on a path, with a file **highlighted** inside its folder
    /// (user ruling, 2026-08-13).
    ///
    /// **A third bridge, and deliberately not a widening of the second.**
    /// [`open_local_path`] hands a path to *whatever the machine has registered
    /// for it* — which is why it reads `PATHEXT` and refuses programs, since
    /// the whole risk there is that opening a thing runs it. This one hands the
    /// path to `explorer.exe` **as text to look at**, and never executes the
    /// target at all: a `.exe` revealed is a `.exe` sitting highlighted in a
    /// folder window, which is precisely what somebody asking "where is this"
    /// wants to see and is not a way to start it. So the extension gate is
    /// absent on purpose, and the shape gate is [`reveal_arguments`]', which is
    /// stricter than its neighbour's in the one way that matters here: what
    /// goes on a command line has to be a path that is really there and a token
    /// Explorer cannot split.
    ///
    /// The one program this can ever launch is Explorer, and it is named
    /// absolutely so that a `explorer.exe` in some working directory cannot be
    /// the one that starts.
    pub fn reveal_in_explorer(window: NativeWindow, path: &Path) -> Result<(), String> {
        let arguments =
            reveal_arguments(path).ok_or_else(|| "path is not one to reveal".to_owned())?;
        hand_explorer_the_argument(window, arguments)
    }

    /// **The same reveal, for a path a worker has already answered for** (closure review of audit
    /// 3 C-2).
    ///
    /// [`reveal_arguments`] asks the disk twice — `metadata` for the file-or-folder question and
    /// `canonicalize` for the rest — and the door above it is reached from a `Ctrl`+click on a
    /// path a *program* printed. That is this branch's own rule broken by its own new code: a
    /// target under a junction into a dead share would stall the window inside `canonicalize`,
    /// on the thread that paints, for the redirector's own timeout.
    ///
    /// So the two facts arrive instead of being fetched. `is_directory` is the ledger's
    /// (`bt_term::PathVerdict::directory`), and the ledger also said the name is there. What
    /// `canonicalize` bought besides those is **text**, and [`reveal_argument_form`] already
    /// answers every text question on its own: a `"`, a control character and a `..` are each
    /// refused there, so an argument Explorer could split never reaches a command line.
    pub fn reveal_verified(
        window: NativeWindow,
        path: &Path,
        target: VerifiedTarget,
    ) -> Result<(), String> {
        if !target.exists {
            return Err("path is not one to reveal".to_owned());
        }
        // `reveal_arguments`' own order, with its two disk calls already made: it validated the
        // *printed* spelling, then built the argument from the **resolved** one. A `None` there
        // means the platform would not resolve it, and the printed name is what this door had
        // before a resolver existed.
        validate_openable_path(path)?;
        let resolved = target
            .resolved
            .clone()
            .unwrap_or_else(|| path.to_path_buf());
        let arguments = reveal_argument_form(&resolved, target.is_directory)
            .ok_or_else(|| "path is not one to reveal".to_owned())?;
        hand_explorer_the_argument(window, arguments)
    }

    /// The one line both reveals end on: `explorer.exe`, named absolutely so that an
    /// `explorer.exe` in some working directory cannot be the one that starts.
    fn hand_explorer_the_argument(
        window: NativeWindow,
        arguments: std::ffi::OsString,
    ) -> Result<(), String> {
        let explorer = windows_directory().join("explorer.exe");
        hand_over(
            window,
            &explorer.to_string_lossy(),
            Some(arguments.as_os_str()),
            &windows_directory(),
        )
    }

    /// **Open Windows' own Fonts page** (user ruling 2026-08-19).
    ///
    /// **A fourth bridge, and the first one that hands `ShellExecuteW` a URI.**
    /// That is exactly the thing this codebase refuses everywhere else —
    /// `preview.rs` will open `http` and `https` and nothing else, because
    /// handing an arbitrary scheme to the shell is handing it whatever the
    /// machine has registered for that scheme. The difference here is that
    /// nothing arbitrary reaches this function: there is no parameter. The two
    /// strings it can pass are both constants of this build, and a reader who
    /// presses `Install fonts…` gets the one page or the other.
    ///
    /// `ms-settings:fonts` first, because it is where a font is installed by
    /// dropping a file on it and where the machine's own fonts already live. It
    /// is a Windows 10+ protocol handler and a machine can have it unregistered
    /// — policy-managed desktops do this — so the fall-back is the folder the
    /// page is a view of, opened through Explorer exactly as
    /// [`reveal_in_explorer`] opens any other folder. Two doors onto one place,
    /// and the reader is never told which one they came through.
    ///
    /// **This product installs and deletes nothing.** A font is a machine-wide
    /// resource, installing one affects every program on the desk, and removing
    /// one a program is drawing with is a decision that was never a terminal's
    /// to take. So there is no in-app font management behind this door and
    /// there will not be — the door is the whole feature.
    pub fn open_system_fonts_page(window: NativeWindow) -> Result<(), String> {
        let system = system_directory();
        if hand_over(window, crate::FONT_SETTINGS_URI, None, &system).is_ok() {
            return Ok(());
        }
        // The URI was refused — no handler, or a policy that removed the page.
        // The folder it is a view of is still there, and Explorer is the one
        // program this fall-back can launch.
        let fonts = crate::fonts_folder();
        let arguments = super::reveal_argument_form(&fonts, true)
            .ok_or_else(|| "no fonts folder".to_owned())?;
        let explorer = windows_directory().join("explorer.exe");
        hand_over(
            window,
            &explorer.to_string_lossy(),
            Some(arguments.as_os_str()),
            &windows_directory(),
        )
    }

    /// The written-down calls of this crate's own tests, one list per thread so
    /// that two tests running at once do not read each other's.
    #[cfg(test)]
    pub(super) mod recorded {
        use std::cell::RefCell;

        thread_local! {
            static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        }

        pub(in super::super) fn note(call: String) {
            CALLS.with(|calls| calls.borrow_mut().push(call));
        }

        /// Everything noted on this thread since the last take, oldest first.
        pub(in super::super) fn take() -> Vec<String> {
            CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN (R1-11) — **the name Windows will resolve is the name the refusal
    /// reads.**
    ///
    /// Win32 strips trailing dots and spaces off the final component before it
    /// opens anything, so `payload.exe.` and `payload.exe ` are the same file
    /// as `payload.exe`. Reading the extension off the untrimmed text let both
    /// of them past a check that then handed the untrimmed text to
    /// `ShellExecuteW`, which trimmed it and ran the program.
    ///
    /// MUTATION: read `Path::extension` off the name as it arrived and the
    /// first two spellings open the door the third one shuts.
    ///
    /// **Gated on its fixture** (`docs/DESIGN.md` §13.6, ticket M1-10): every
    /// string below is a Windows path and the rule itself is Win32's own
    /// normalisation. `a_posix_name_is_its_bytes_and_nothing_is_trimmed` is the
    /// mirror, and it is a different rule rather than the same one restated —
    /// which is the point of writing it.
    #[cfg(windows)]
    #[test]
    fn a_trailing_dot_or_space_does_not_hide_what_a_name_would_run() {
        for program in [
            r"C:\bin\payload.exe.",
            "C:\\bin\\payload.exe ",
            r"C:\bin\payload.exe. . ",
            r"C:\bin\install.msi.",
            "C:\\bin\\shortcut.lnk ",
            // A name that is nothing but an extension is that extension to
            // Windows, and this window does not open it either.
            r"C:\bin\.exe",
        ] {
            assert!(
                names_a_program(Path::new(program), ""),
                "{program:?} is the program Windows would resolve"
            );
        }
        // The trim does not invent an extension where the name has none: a file
        // called `readme.` is `readme`, which is nothing to run.
        for document in [
            r"C:\notes\readme.",
            "C:\\notes\\readme ",
            r"C:\notes\NOEXTENSION",
        ] {
            assert!(
                !names_a_program(Path::new(document), ""),
                "{document:?} is something to look at"
            );
        }
        // And it does not change what the name *is* for anything else.
        assert!(names_a_program(Path::new(r"C:\notes\a.md."), ".MD"));
        assert!(!names_a_program(Path::new(r"C:\notes\a.md."), ""));
        assert_eq!(
            normalised_target(Path::new(r"C:\bin\payload.exe. ")),
            Some(PathBuf::from(r"C:\bin\payload.exe"))
        );
    }

    /// PIN — **the POSIX mirror of the rule above, and the rule is that there
    /// is none: a name here is its bytes** (M1-10, `docs/DESIGN.md` §13.6).
    ///
    /// The Windows test next door exists because Win32 resolves `payload.exe.`
    /// and `payload.exe` to one file, so a check that read the untrimmed text
    /// and a call that read the trimmed text were talking about two things. A
    /// POSIX filesystem has no such rule: `payload.` is a name with a dot at the
    /// end of it, `payload ` is a name with a space at the end of it, and each
    /// is a different file from `payload`. Writing the mirror is what makes that
    /// difference a decision somebody took rather than a gap.
    ///
    /// Two claims, both about this module's own functions:
    ///
    /// ① [`program_in_directories`] — the portable half of the search — looks
    ///    for the name exactly as it was written, so `payload.sh.` and
    ///    `payload.sh` look for two different files.
    /// ② the question *this* module would otherwise answer with
    ///    [`names_a_program`] is not answered here at all: off Windows a program
    ///    is one the execute bit says is a program, `PATHEXT` names nothing, and
    ///    `program_on_path` refuses rather than guessing (M2-2 read that door
    ///    and left the refusal standing; its own note says why). A refusal is
    ///    the honest answer; a Windows reading of a Unix name would be a wrong
    ///    one.
    ///
    /// MUTATION: build the candidate out of [`effective_final_component`]
    /// instead of out of the name and the second half goes green, which is the
    /// search quietly opening a file nobody named.
    #[cfg(not(windows))]
    #[test]
    fn a_posix_name_is_its_bytes_and_nothing_is_trimmed() {
        let directories = [PathBuf::from("/usr/local/bin")];
        let only_the_plain_one = |path: &Path| path == Path::new("/usr/local/bin/payload.sh");
        assert_eq!(
            program_in_directories(
                Path::new("payload.sh"),
                &directories,
                "",
                &only_the_plain_one
            ),
            Some(PathBuf::from("/usr/local/bin/payload.sh")),
            "the name as it is written is the file that is looked for"
        );
        for trailing in ["payload.sh.", "payload.sh "] {
            assert_eq!(
                program_in_directories(Path::new(trailing), &directories, "", &only_the_plain_one),
                None,
                "{trailing:?} is a name of its own here and no trim turns it into `payload.sh`"
            );
        }

        // And the program question itself is refused rather than answered with
        // Windows' grammar.
        assert_eq!(program_on_path(Path::new("payload.sh")), None);
    }

    /// PIN — the tree's bridge opens documents and refuses programs, and the
    /// refusal does not depend on this machine's `PATHEXT` being anything in
    /// particular.
    #[test]
    fn the_tree_bridge_opens_what_it_can_show_and_never_what_it_would_run() {
        let empty = "";
        for document in [
            r"C:\notes\readme.md",
            r"C:\notes\report.pdf",
            r"C:\notes\archive.zip",
            r"C:\notes\NOEXTENSION",
            r"C:\notes\photo.PNG",
        ] {
            assert!(
                !names_a_program(Path::new(document), empty),
                "{document} is something to look at"
            );
        }
        for program in [
            r"C:\bin\tool.exe",
            r"C:\bin\TOOL.EXE",
            r"C:\bin\run.bat",
            r"C:\bin\install.msi",
            r"C:\bin\shortcut.lnk",
            r"C:\bin\saver.scr",
            r"C:\bin\page.hta",
            r"C:\bin\keys.reg",
            r"C:\bin\script.ps1",
        ] {
            assert!(
                names_a_program(Path::new(program), empty),
                "{program} would run"
            );
        }
    }

    /// PIN — a machine that has taught its command line to execute a new
    /// extension has taught this bridge to refuse it.
    #[test]
    fn a_machine_that_makes_something_executable_makes_it_refused_here() {
        let path = Path::new(r"C:\bin\macro.xyz");
        assert!(!names_a_program(path, ".EXE;.BAT"));
        assert!(names_a_program(path, ".EXE;.XYZ"));
        assert!(names_a_program(path, ".exe;.xyz"));
        // An emptied `PATHEXT` cannot open the door the fixed list shuts.
        assert!(names_a_program(Path::new(r"C:\bin\tool.exe"), ""));
    }

    /// PIN (R1-11) — **the shapes that ask Windows not to normalise are not
    /// paths this window hands over.**
    ///
    /// `\\?\` and `\\.\` reach the object manager with the text intact, so
    /// `\\?\C:\payload.exe.` really is a file whose name ends in a dot and the
    /// trim above would be answering about a different file. Neither prefix is
    /// a thing a files column, a git pathname or a printed reference ever
    /// produces, so both are refused rather than reasoned about.
    #[test]
    fn the_verbatim_and_device_spellings_are_not_opened_at_all() {
        for path in [
            r"\\?\C:\bin\payload.exe.",
            r"\\.\C:\bin\payload.exe",
            r"\\?\UNC\server\share\a.txt",
            r"\??\C:\bin\payload.exe",
        ] {
            assert!(
                asks_windows_not_to_normalise(Path::new(path)),
                "{path} asks Windows to skip its own normalisation"
            );
            assert!(validate_openable_path(Path::new(path)).is_err());
            assert!(reveal_argument_form(Path::new(path), false).is_none());
        }
        assert!(!asks_windows_not_to_normalise(Path::new(
            r"C:\bin\tool.exe"
        )));
        assert!(!asks_windows_not_to_normalise(Path::new(
            r"\\server\share\a.txt"
        )));
    }

    /// PIN (R1-5) — **what Explorer is handed is one token it cannot split.**
    ///
    /// `/select,` takes a single argument and the path is quoted to make it
    /// one. A quote inside the name ends the quoted run and the rest of the
    /// name becomes Explorer's next argument; `..` makes the text and the place
    /// two different questions. Both are refusals, and a refusal here has
    /// nothing to hand over.
    ///
    /// MUTATION: wrap whatever arrives in quotes and the first case hands
    /// Explorer two arguments.
    ///
    /// **Gated on its fixture** (`docs/DESIGN.md` §13.6, ticket M1-10): the
    /// paths are Windows paths and the answer is an Explorer command line.
    /// `the_posix_reveal_hands_over_a_path_and_not_a_command_line` is the
    /// mirror.
    #[cfg(windows)]
    #[test]
    fn the_reveal_argument_is_one_token_or_it_is_nothing() {
        assert_eq!(
            reveal_argument_form(Path::new("C:\\bin\\a\"b.txt"), false),
            None
        );
        assert_eq!(
            reveal_argument_form(Path::new("C:\\bin\\a\u{7}b.txt"), false),
            None
        );
        assert_eq!(
            reveal_argument_form(Path::new(r"C:\bin\..\other\a.txt"), false),
            None
        );
        assert_eq!(reveal_argument_form(Path::new(r"notes\a.txt"), false), None);

        // A comma and a trailing space are ordinary characters inside the one
        // quoted token, and they arrive whole.
        let arguments = reveal_argument_form(Path::new(r"C:\bin\a,b .txt"), false)
            .expect("an ordinary file")
            .to_string_lossy()
            .into_owned();
        assert_eq!(arguments, "/select,\"C:\\bin\\a,b .txt\"");
        assert_eq!(arguments.matches('"').count(), 2);

        // A folder is opened rather than selected.
        assert_eq!(
            reveal_argument_form(Path::new(r"C:\bin"), true)
                .expect("a folder")
                .to_string_lossy(),
            "\"C:\\bin\""
        );
    }

    /// PIN — **the POSIX mirror: there is no command line to be split, so the
    /// quoting question does not arise and the door says so** (M1-10,
    /// `docs/DESIGN.md` §13.6).
    ///
    /// What `open -R <path>` takes — and what
    /// `-[NSWorkspace activateFileViewerSelectingURLs:]` takes, which is what
    /// M2-2's arm really calls — is **one element of an `argv` array, or one
    /// `NSURL`**. Nothing between this process and Finder re-parses a string,
    /// so a quote in a file name is an ordinary character and `/select,` has no
    /// counterpart at all: the whole of [`reveal_argument_form`]'s subject is a
    /// Windows fact about `ShellExecuteW`'s parameter being a command line.
    ///
    /// What does carry over is the *other* refusal, and this pins that the door
    /// keeps it rather than quietly calling `open` on anything: off Windows
    /// `reveal_in_explorer` answers a refusal with a reason a toast can carry,
    /// and that reason is not [`PROGRAM_REFUSED`] — "the machine was never
    /// asked" and "this window will not run programs" are two different
    /// sentences and the files column matches on the second.
    ///
    /// **Not on macOS**, where the door is written and the same two paths are a
    /// real reveal and a real refusal —
    /// `macos_handoff::tests::a_reveal_asks_the_disk_before_it_asks_finder`
    /// asks this question of that arm.
    ///
    /// MUTATION: make the portable arm answer `Ok(())` and the first assertion
    /// goes red, which is a reveal that reports success and shows nothing.
    #[cfg(all(not(windows), not(target_os = "macos")))]
    #[test]
    fn the_posix_reveal_hands_over_a_path_and_not_a_command_line() {
        let window = crate::NativeWindow::stand_in(0);
        // A name a Windows command line could not carry as one token is an
        // ordinary name here, and the refusal that comes back is about the
        // platform rather than about the text.
        let refusal = reveal_in_explorer(window, Path::new("/Users/a/a\"b .txt"))
            .expect_err("the file manager has not been asked yet on this platform");
        assert!(
            !refusal.is_empty() && refusal != PROGRAM_REFUSED,
            "a refusal carries its own reason: {refusal:?}"
        );
        assert_eq!(
            reveal_in_explorer(window, Path::new("/Users/a")),
            Err(refusal),
            "a folder is the same door and the same answer"
        );
    }

    /// PIN (R1-5) — **and it is a path that is there.**
    ///
    /// A path that names nothing leaves Explorer to fall back to a folder
    /// nobody asked for, which reads as this window having opened the wrong
    /// thing. Canonicalising is what answers both that question and the `..`
    /// one, in the operating system's own words.
    #[cfg(windows)]
    #[test]
    fn a_reveal_names_a_path_that_is_really_there() {
        let scratch =
            std::env::temp_dir().join(format!("folio-reveal-argument-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("a scratch directory");
        let file = scratch.join("a,b .txt");
        std::fs::write(&file, b"x").expect("a scratch file");

        let arguments = reveal_arguments(&file)
            .expect("a file that is there")
            .to_string_lossy()
            .into_owned();
        assert!(arguments.starts_with("/select,\""), "{arguments}");
        assert!(arguments.ends_with('"'), "{arguments}");
        assert_eq!(arguments.matches('"').count(), 2, "{arguments}");
        assert!(arguments.contains("a,b .txt"), "{arguments}");
        assert!(!arguments.contains(r"\\?\"), "{arguments}");

        // The folder is opened rather than selected, and it is read off the
        // disk rather than taken on the caller's word.
        let folder = reveal_arguments(&scratch)
            .expect("a folder that is there")
            .to_string_lossy()
            .into_owned();
        assert!(!folder.starts_with("/select,"), "{folder}");

        assert_eq!(reveal_arguments(&scratch.join("not-here.txt")), None);
        assert_eq!(reveal_arguments(Path::new(r"notes\a.txt")), None);

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&scratch);
    }

    /// PIN (R1-17) — **a program this product starts is found in a place
    /// somebody administers, and the working directory is not one.**
    ///
    /// MUTATION: put the working directory at the head of `directories` and the
    /// last assertion finds the planted program.
    ///
    /// **Gated on its fixture** (`docs/DESIGN.md` §13.6, ticket M1-10): the
    /// directories are Windows directories, the extension list is a `PATHEXT`
    /// and `C:\Windows\System32\cmd.exe` is only absolute on a machine with
    /// drive letters. `a_posix_program_is_looked_for_where_an_administrator_put_it`
    /// is the mirror and it asks the same question of `/usr/local/bin`.
    #[cfg(windows)]
    #[test]
    fn a_program_is_looked_for_where_an_administrator_put_it() {
        let directories = [
            PathBuf::from(r"C:\Windows\System32"),
            PathBuf::from(r"C:\Users\a\AppData\Roaming\npm"),
        ];
        let there = |path: &Path| {
            matches!(
                path.to_string_lossy().as_ref(),
                r"C:\Windows\System32\cmd.exe"
                    | r"C:\Users\a\AppData\Roaming\npm\copilot.cmd"
                    | r"D:\somebody\clone\copilot.cmd"
            )
        };
        // A name with no extension is tried against `PATHEXT`, in the order the
        // machine wrote it, one directory at a time.
        assert_eq!(
            program_in_directories(
                Path::new("copilot"),
                &directories,
                ".COM;.EXE;.BAT;.CMD",
                &there
            ),
            Some(PathBuf::from(r"C:\Users\a\AppData\Roaming\npm\copilot.cmd"))
        );
        // A name that carries its own extension is taken as it is.
        assert_eq!(
            program_in_directories(Path::new("cmd.exe"), &directories, ".COM;.EXE", &there),
            Some(PathBuf::from(r"C:\Windows\System32\cmd.exe"))
        );
        // An absolute program is already an answer and is not searched for.
        assert_eq!(
            program_in_directories(Path::new(r"C:\Windows\System32\cmd.exe"), &[], "", &|_| {
                true
            }),
            Some(PathBuf::from(r"C:\Windows\System32\cmd.exe"))
        );
        // A relative path resolves against a working directory this window does
        // not choose, so it is not a program name at all.
        assert_eq!(
            program_in_directories(Path::new(r"tools\copilot.cmd"), &directories, "", &|_| true),
            None
        );
        // And the folder the reader is standing in is never searched, even when
        // it holds exactly the name that was asked for.
        assert_eq!(
            program_in_directories(Path::new("copilot"), &directories, ".CMD", &|path| path
                .to_string_lossy()
                == r"D:\somebody\clone\copilot.cmd"),
            None
        );
    }

    /// PIN — **the POSIX mirror: the order is the administrator's directories in
    /// the order `PATH` names them, and the working directory is not one of
    /// them** (M1-10, `docs/DESIGN.md` §13.6).
    ///
    /// The rule R1-17 wrote is not about Windows: `CreateProcess` searching the
    /// current directory is the Windows spelling of a hazard every shell has a
    /// name for, and a `copilot` dropped in a cloned repository is the same
    /// attack on a Mac. So the search order is asserted here with the
    /// directories a Unix machine actually has — `/usr/local/bin` before
    /// `/usr/bin`, which is `PATH` order on macOS — and with the cloned
    /// repository left out of the list rather than trusted.
    ///
    /// **And the one thing that does not carry over is pinned too.** The
    /// candidate spellings this function tries come from `PATHEXT`, which exists
    /// nowhere but Windows, so an extension-less `copilot` — the ordinary shape
    /// of a Unix program — produces no candidate at all and is not found. That
    /// is why [`program_on_path`] refuses off Windows instead of calling this:
    /// the POSIX question is the execute bit, and no caller off Windows asks it
    /// yet — M2-2 wrote that reasoning onto the door rather than writing a
    /// search nothing reaches. A green test that quietly resolved a bare name
    /// here would be this module claiming a rule it has not been given.
    ///
    /// MUTATION: put `/home/a/clone` at the head of `directories` and the last
    /// assertion of the first block finds the planted program, which is the
    /// defect itself.
    #[cfg(not(windows))]
    #[test]
    fn a_posix_program_is_looked_for_where_an_administrator_put_it() {
        let directories = [PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin")];
        let there = |path: &Path| {
            matches!(
                path.to_string_lossy().as_ref(),
                "/usr/local/bin/copilot.sh" | "/usr/bin/copilot.sh" | "/home/a/clone/copilot.sh"
            )
        };
        // The first directory that has it wins, which is `PATH` order.
        assert_eq!(
            program_in_directories(Path::new("copilot.sh"), &directories, "", &there),
            Some(PathBuf::from("/usr/local/bin/copilot.sh"))
        );
        assert_eq!(
            program_in_directories(
                Path::new("copilot.sh"),
                &directories[1..],
                "",
                &|path: &Path| path == Path::new("/usr/bin/copilot.sh")
            ),
            Some(PathBuf::from("/usr/bin/copilot.sh"))
        );
        // An absolute program is already an answer and is not searched for.
        assert_eq!(
            program_in_directories(Path::new("/usr/bin/env"), &[], "", &|_| true),
            Some(PathBuf::from("/usr/bin/env"))
        );
        // A relative path resolves against a working directory this window does
        // not choose, so it is not a program name at all.
        assert_eq!(
            program_in_directories(Path::new("tools/copilot.sh"), &directories, "", &|_| true),
            None
        );
        // And the folder the reader is standing in is never searched, even when
        // it holds exactly the name that was asked for.
        assert_eq!(
            program_in_directories(Path::new("copilot.sh"), &directories, "", &|path: &Path| {
                path == Path::new("/home/a/clone/copilot.sh")
            }),
            None
        );

        // The half that is Windows' and stays there: with no `PATHEXT` there is
        // no spelling to try for a name that carries no extension.
        assert_eq!(
            program_in_directories(Path::new("copilot"), &directories, "", &|_| true),
            None,
            "the extension list is a Windows fact; the execute bit is M2-2's"
        );
        assert_eq!(program_on_path(Path::new("copilot")), None);
    }

    /// RED (ticket 10, owner 2026-09-22) — **the reveal grants the foreground before it hands
    /// over, and so does an open.**
    ///
    /// Explorer opened *behind* Folio on a `Ctrl`+click of a printed folder: `explorer.exe
    /// /select,…` forwards the request to the Explorer already running and exits, and that
    /// Explorer has no right to the front unless the foreground process granted one before the
    /// hand-off. The grant after the call is too late — the forward has already happened — so
    /// the order is the claim. What this can hold is the order and the exact argument handed
    /// over, through this crate's recorded seam (`windows_handoff::shell_execute_w` writes the
    /// call down in tests instead of making it); what it cannot hold is Explorer's own
    /// behaviour, which is the owner's machine's measurement.
    ///
    /// Run through [`ShellThread`], the lane's own entry, on a thread the door started (the only
    /// thread that can enter one), over a real file and a real folder, so the argument is the real
    /// reveal's (`reveal_arguments` asked of the disk) and not a string written here.
    ///
    /// MUTATION: delete the `let_the_receiver_take_the_front();` line from `hand_over`, or move
    /// it after `shell_execute_w`, and the first entry is no longer the grant.
    #[cfg(windows)]
    #[test]
    fn the_reveal_grants_the_foreground_before_it_hands_over() {
        let scratch =
            std::env::temp_dir().join(format!("folio-handoff-front-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("a scratch directory");
        let file = scratch.join("notes.md");
        std::fs::write(&file, b"x").expect("a scratch file");
        let window = crate::NativeWindow::stand_in(0);
        let (lane_file, lane_scratch) = (file.clone(), scratch.clone());
        let lane = crate::spawn_at_priority(
            "bt-test-handoff",
            crate::ThreadPriority::BelowNormal,
            move |ctx| {
                let (file, scratch) = (lane_file, lane_scratch);
                let shell = ShellThread::enter(ctx);
                let _ = windows_handoff::recorded::take();

                for request in [
                    Handoff::Reveal(file.clone()),
                    Handoff::Reveal(scratch.clone()),
                    Handoff::Open(file.clone()),
                ] {
                    assert_eq!(shell.hand_over(window, &request), Ok(()), "{request:?}");
                    let calls = windows_handoff::recorded::take();
                    assert_eq!(calls.len(), 2, "one grant, one hand-off: {calls:?}");
                    assert_eq!(
                        calls[0], "AllowSetForegroundWindow(ASFW_ANY)",
                        "the grant comes first, or the receiver cannot take the front: {calls:?}"
                    );
                    assert!(calls[1].starts_with("ShellExecuteW "), "{calls:?}");
                }
                // The grant is not a second decision: a refused hand-off grants nothing, because
                // the refusal is the door's and comes before the call.
                let program = scratch.join("payload.exe");
                assert_eq!(
                    shell.hand_over(window, &Handoff::Open(program)),
                    Err(PROGRAM_REFUSED.to_owned())
                );
                assert!(windows_handoff::recorded::take().is_empty());
            },
        )
        .expect("the door starts a thread");
        if let Err(panic) = lane.join() {
            std::panic::resume_unwind(panic);
        }

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&scratch);
    }

    /// RED (A1b, worker-side M5) — **the hand-off is reached through an indirection only when
    /// the indirection is handed the capability, and then it is the real door that answers.**
    ///
    /// A boxed closure and a function pointer, each typed `(&WorkerCtx)`, run inside a body the
    /// thread door started and pass the lent capability on to [`ShellThread::enter`]; the entered
    /// value hands a relative name — which every platform's door refuses before it asks the
    /// machine anything — to the door it names. Both roads come back with the door's own refusal,
    /// the same words, on a thread whose role is the worker's. (Their compile-time twins, the same
    /// indirections typed with no capability, are the `compile_fail` pairs on
    /// [`crate::admission::spawn_at_priority`].)
    ///
    /// MUTATION: have `ShellThread::door` answer `Ok(())` for `Handoff::Open` instead of calling
    /// the verb, and the refusal assertion goes red.
    #[test]
    fn a_boxed_closure_and_a_function_pointer_reach_the_hand_off_only_with_the_capability() {
        type HandOver = dyn Fn(&WorkerCtx) -> Result<(), String>;
        fn through_a_pointer(ctx: &WorkerCtx) -> Result<(), String> {
            ShellThread::enter(ctx).hand_over(
                crate::NativeWindow::stand_in(0),
                &Handoff::Open(PathBuf::from("relative-name.md")),
            )
        }
        let worker = crate::spawn_at_priority(
            "bt-test-indirect-handoff",
            crate::ThreadPriority::BelowNormal,
            |ctx| {
                let boxed: Box<HandOver> = Box::new(|ctx| {
                    ShellThread::enter(ctx).hand_over(
                        crate::NativeWindow::stand_in(0),
                        &Handoff::Open(PathBuf::from("relative-name.md")),
                    )
                });
                let pointer: fn(&WorkerCtx) -> Result<(), String> = through_a_pointer;
                (crate::admission::role(), boxed(ctx), pointer(ctx))
            },
        )
        .expect("the door starts a thread");
        let (role, boxed, pointer) = worker.join().expect("the worker ran");
        assert_eq!(
            role,
            crate::admission::Role::Worker("bt-test-indirect-handoff")
        );
        let refusal = boxed.expect_err("a relative name is refused by every platform's door");
        assert!(!refusal.is_empty(), "the door says why");
        assert_eq!(pointer, Err(refusal), "both roads reach the same door");
    }
}
