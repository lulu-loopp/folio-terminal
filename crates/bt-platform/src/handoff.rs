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

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The refusal's own words, so the caller can tell "this window will not do
/// that" apart from "Windows could not".
pub const PROGRAM_REFUSED: &str = "the files tree does not run programs";

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
    let canonical = std::fs::canonicalize(path).ok()?;
    let canonical = strip_verbatim_prefix(&canonical);
    reveal_argument_form(&canonical, metadata.is_dir())
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

    /// Show a file in the file manager.
    #[cfg(not(target_os = "macos"))]
    pub fn reveal_in_explorer(window: NativeWindow, path: &Path) -> Result<(), String> {
        let _ = (window, path);
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
    open_local_file, open_local_path, open_system_fonts_page, reveal_in_explorer, shell_execute,
};

/// **The five verbs that leave this window, over `NSWorkspace`** (M2-2).
#[cfg(target_os = "macos")]
pub use macos_handoff::{
    open_local_file, open_local_path, open_system_fonts_page, reveal_in_explorer, shell_execute,
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
/// `openURL:configuration:completionHandler:` beside it. That is the same
/// bargain the Windows arm takes — `ShellExecuteW` blocks the window thread
/// too — and taking the asynchronous form would mean a completion block, a
/// package (`block2`) and a second answer arriving after the caller has already
/// been told `Ok`. A door whose `Result` is a real answer is worth the wait
/// this one costs.
///
/// # The window is spare, and stays in the signature
///
/// Each of these takes a `NativeWindow` because `ShellExecuteW` takes an
/// `HWND` — the window an error box is parented to. `NSWorkspace` has nothing
/// to be given one. The parameter stays because a door in this crate has one
/// signature on every platform (§4.4 ②, M1-9), and it is consumed with
/// `let _ = window;` at the top of each body so that a reader meets the fact
/// rather than deducing it.
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

    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSString, NSURL, ns_string};

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
    /// Scheme allowlisting deliberately belongs to the caller — in this product
    /// that is `webnav::address_bar`, which every caller passes through — and
    /// this bridge supplies the parse. `URLWithString:` is that parse, and its
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

    /// Open one file the user picked out of a directory listing with its
    /// registered default handler — and never run a program.
    ///
    /// The two gates are [`openable_unix_path`] and [`opening_it_would_run_it`],
    /// and the second is the product rule the files column matches on:
    /// [`PROGRAM_REFUSED`] is the sentence `bt-app` turns into *the tree does
    /// not run programs*, and every other refusal here is a fact about the
    /// machine that the same caller shows as a toast.
    pub fn open_local_path(window: NativeWindow, path: &Path) -> Result<(), String> {
        let _ = window;
        openable_unix_path(path)?;
        let metadata = std::fs::metadata(path).map_err(|error| format!("{path:?}: {error}"))?;
        if opening_it_would_run_it(path, &metadata) {
            return Err(PROGRAM_REFUSED.to_owned());
        }
        let url = file_url(path, metadata.is_dir())?;
        hand_over(&url, &path.to_string_lossy())
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
        let url = file_url(&real, directory)?;
        let workspace = NSWorkspace::sharedWorkspace();
        if directory {
            // A folder is *opened*, not selected in its parent. `openURL:` on a
            // folder is what Finder does with a double click, and it answers
            // whether it took it — which is more than the viewer call can say.
            return hand_over(&url, &real.to_string_lossy());
        }
        workspace.activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
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
        use std::path::PathBuf;

        use objc2::rc::Retained;
        use objc2_app_kit::NSRunningApplication;

        use super::*;

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
            let _ = std::fs::remove_dir_all(&root);
        }

        /// RED — **a file handed to the workspace really opens**, which is the
        /// one claim about LaunchServices that nothing else in this file can
        /// make.
        ///
        /// **It ends what it started, and only that.** The applications running
        /// before the call are written down; afterwards the one that is not
        /// among them is the one this test launched, and it is asked to quit by
        /// its own object. Apple's note on `processIdentifier` says to compare
        /// with `isEqual:` rather than with a pid, and `isEqual:` is what the
        /// search below uses — so a TextEdit the owner already had open cannot
        /// be matched, because it is in the *before* list.
        ///
        /// If nothing new appears — the handler was already running and reused
        /// its process — there is nothing to end and nothing is ended. The
        /// assertion is on the door's answer, not on a new process, so that is
        /// not a failure.
        #[test]
        fn open_local_path_hands_a_file_to_the_workspace() {
            let window = crate::NativeWindow::stand_in(0);
            let root = scratch("open");
            let file = root.join("folio-m2-2.txt");
            std::fs::write(&file, b"M2-2 opened this.\n").expect("a document");

            let workspace = NSWorkspace::sharedWorkspace();
            let before: Vec<Retained<NSRunningApplication>> =
                workspace.runningApplications().to_vec();

            assert_eq!(
                open_local_path(window, &file),
                Ok(()),
                "the workspace took the document"
            );

            // LaunchServices answers before the application has finished
            // launching, so the new process is waited for rather than assumed.
            let mut opened: Option<Retained<NSRunningApplication>> = None;
            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                opened = workspace
                    .runningApplications()
                    .to_vec()
                    .into_iter()
                    .find(|running| !before.iter().any(|known| **known == **running));
                if opened.is_some() {
                    break;
                }
            }
            if let Some(opened) = opened {
                opened.terminate();
            }
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}

/// The Windows half: the real directories, the real `PATHEXT`, the real disk.
#[cfg(windows)]
pub use windows_handoff::{
    open_local_file, open_local_path, open_system_fonts_page, program_on_path, reveal_in_explorer,
    shell_execute,
};

#[cfg(windows)]
mod windows_handoff {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use crate::NativeWindow;
    use windows::Win32::Foundation::MAX_PATH;
    use windows::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

    use super::{
        DEFAULT_PATHEXT, PROGRAM_REFUSED, names_a_program, normalised_target,
        program_in_directories, reveal_arguments, validate_local_image_path,
        validate_openable_path,
    };

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
    fn hand_over(
        window: NativeWindow,
        program: &str,
        arguments: Option<&std::ffi::OsStr>,
        directory: &Path,
    ) -> Result<(), String> {
        let hwnd = window.as_hwnd();
        let mut operation = wide("open");
        let mut program = wide(program);
        let mut arguments = arguments.map(wide_os);
        let mut directory = wide_os(directory.as_os_str());
        // SAFETY: every buffer below is a live, NUL-terminated UTF-16 buffer
        // for the duration of this synchronous call, the caller's gate has
        // refused any embedded NUL, and `hwnd` is winit's live top-level
        // window.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
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

    fn wide(text: &str) -> Vec<u16> {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        units.push(0);
        units
    }

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
    /// Scheme allowlisting deliberately belongs to the caller — in this product
    /// that is `webnav::address_bar`, which every caller passes through — and
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
        if names_a_program(&path, std::env::var("PATHEXT").unwrap_or_default().as_str()) {
            return Err(PROGRAM_REFUSED.to_owned());
        }
        hand_over(window, &path.to_string_lossy(), None, &folder_of(&path))
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
}
