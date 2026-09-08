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

/// The Windows half: the real directories, the real `PATHEXT`, the real disk.
#[cfg(windows)]
pub use windows_handoff::{
    open_local_file, open_local_path, open_system_fonts_page, program_on_path, reveal_in_explorer,
    shell_execute,
};

#[cfg(windows)]
mod windows_handoff {
    use std::ffi::c_void;
    use std::num::NonZeroIsize;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use windows::Win32::Foundation::{HWND, MAX_PATH};
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
        hwnd: NonZeroIsize,
        program: &str,
        arguments: Option<&std::ffi::OsStr>,
        directory: &Path,
    ) -> Result<(), String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
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
    pub fn shell_execute(hwnd: NonZeroIsize, target: &str) -> Result<(), String> {
        if target.contains('\0') {
            return Err("ShellExecuteW target contains an embedded NUL".to_owned());
        }
        hand_over(hwnd, target, None, &system_directory())
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
    pub fn open_local_file(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        let path = normalised_target(path).ok_or_else(|| "path has no name".to_owned())?;
        validate_local_image_path(&path)?;
        hand_over(hwnd, &path.to_string_lossy(), None, &folder_of(&path))
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
    pub fn open_local_path(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        let path = normalised_target(path).ok_or_else(|| "path has no name".to_owned())?;
        validate_openable_path(&path)?;
        if names_a_program(&path, std::env::var("PATHEXT").unwrap_or_default().as_str()) {
            return Err(PROGRAM_REFUSED.to_owned());
        }
        hand_over(hwnd, &path.to_string_lossy(), None, &folder_of(&path))
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
    pub fn reveal_in_explorer(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        let arguments =
            reveal_arguments(path).ok_or_else(|| "path is not one to reveal".to_owned())?;
        let explorer = windows_directory().join("explorer.exe");
        hand_over(
            hwnd,
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
    pub fn open_system_fonts_page(hwnd: NonZeroIsize) -> Result<(), String> {
        let system = system_directory();
        if hand_over(hwnd, crate::FONT_SETTINGS_URI, None, &system).is_ok() {
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
            hwnd,
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
}
