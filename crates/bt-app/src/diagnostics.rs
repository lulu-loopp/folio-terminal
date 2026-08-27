//! **Where this process says things, and when that changes.**
//!
//! # The fault
//!
//! `folio.exe` is a windows-subsystem binary (`main.rs`'s first line), so the
//! loader hands it no console — and the very first statement of `main` is
//! [`bt_platform::adopt_parent_console`], which borrows the console of whoever
//! launched it and points the null `stdout`/`stderr` slots at that screen. The
//! motive was right and is still right: a `--help` has to reach the person who
//! typed it, a refused flag has to say why, and a developer who set
//! `BT_STARTUP_TRACE` from a shell has to see the trace in that shell (user
//! report, 2026-08-18).
//!
//! What was wrong was the **lifetime**. The borrow was for the life of the
//! process, and the console a Folio adopts is very often a pane inside a
//! *running* Folio. So every resident diagnostic this workspace writes — some
//! two hundred and forty `eprintln!` across fourteen crates — was landing in
//! the middle of somebody's shell session. The user saw it as `Folio's window
//! thread has not answered for 5.748s` appearing inside a Claude Code input box,
//! every eight seconds, from a window that was merely idle.
//!
//! # The line this file draws
//!
//! > **The synchronous answer to the command somebody just typed belongs on the
//! > console. A resident asynchronous diagnostic belongs in a log file.**
//!
//! Which is a statement about *when*, not about *what*: the same `eprintln!`
//! is right on the console during the front door and wrong on it a second
//! later. So the console is kept for the front door — argument parsing, a
//! refusal, `--help` — and at the moment the process commits to running,
//! [`enter_resident_run`] moves `stdout` and `stderr` to a file under
//! `%APPDATA%\Folio\` and lets the console go.
//!
//! **Except when the run asked for the console**, which is what the trace
//! variables are: `BT_STARTUP_TRACE`, `BT_MOUSE_TRACE`, `BT_WEB_TRACE_V` and
//! the rest of that family exist to be watched from a shell, and a person who
//! sets one has named the console as the destination. The rule is the family
//! and not a list — any `BT_…TRACE…` in the environment — because a list is a
//! thing the next trace variable gets left off. `BT_PTY_DUMP` deliberately does
//! **not** qualify: it names a file of its own, it asks for nothing on a screen,
//! and it is the one variable the project's own test windows always carry, which
//! would have reinstated the fault in exactly the case that reported it.
//!
//! # Letting the console go is also the fix for a second thing
//!
//! `AttachConsole` does not only open a screen; it puts this process into that
//! console's **process group**, which is who `CTRL_C_EVENT` and
//! `CTRL_CLOSE_EVENT` are delivered to, and the default handler for both is to
//! terminate. A terminal emulator that dies because somebody closed the shell it
//! was launched from has a fatal relationship with its own parent. `FreeConsole`
//! ends the membership; [`bt_platform::install_console_ctrl_handler`] covers the
//! window before it and the whole of a run that keeps the console on purpose.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The file resident diagnostics are appended to, beside `hang-reports\`.
pub const LOG_FILENAME: &str = "diagnostics.log";

/// The previous log, kept across exactly one rotation.
pub const PREVIOUS_LOG_FILENAME: &str = "diagnostics.prev.log";

/// **How large the log may be when a run starts before it is rotated.**
///
/// Four mebibytes, and the cap is checked once — at startup, in
/// [`rotate_if_oversized`] — rather than policed on every write. That is the
/// simplest policy that is also honest about what it promises: the disk this
/// facility can occupy is two files, so at most this much of history plus
/// whatever the *current* run writes. Bounding a single run's output would mean
/// putting a size-counting writer between `eprintln!` and the handle, and the
/// whole design here is that there is nothing between them — `SetStdHandle`
/// moves the channel, so no call site has to know it moved.
///
/// The previous run is kept rather than truncated because the run before the
/// one you are debugging is very often the one that crashed.
pub const LOG_ROTATE_AT: u64 = 4 * 1024 * 1024;

/// Where this process's `stdout` and `stderr` point once the front door has
/// closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Channel {
    /// The console that started this process, kept because the run asked for
    /// it. See [`console_was_asked_for`].
    Console,
    /// The log file under `%APPDATA%\Folio\`. The ordinary answer.
    Log,
    /// Nowhere at all: the log could not be opened. **Never the console** — the
    /// console is the one destination that belongs to somebody else, and a
    /// failure to open a file is not a reason to start writing on their screen.
    Nowhere,
}

impl Channel {
    /// The word a startup trace prints for this channel.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Console => "the console that started it",
            Self::Log => "its log file",
            Self::Nowhere => "nowhere — its log could not be opened",
        }
    }
}

/// **What a `BT_…` variable names, when it names a file.**
///
/// An emptied variable is **off**, not a file called the empty string. That is
/// the standing rule for every environment variable this program reads, and it
/// is written here as one function rather than as a `filter` remembered at each
/// call site, because the failure it prevents is silent at the site that forgets
/// it: `BT_PROBE_INPUT=` stopped the whole program before its window with `read
/// BT_PROBE_INPUT : The system cannot find the path specified. (os error 3)`,
/// and `BT_PTY_DUMP=` did the same to a pane before it. That is the shape a
/// shell leaves behind when it *clears* a variable rather than removing it, so
/// the failure lands on exactly the people who believed they had switched the
/// diagnostic off.
///
/// Whitespace is deliberately **not** trimmed: `" "` is a strange but real
/// relative filename on Windows' rules, and a diagnostic that quietly rewrites
/// the path it was handed is the same class of surprise in the other direction.
#[must_use]
pub fn named_file(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// **Whether a `BT_…` switch is on**, read from its value rather than from its
/// presence.
///
/// The other half of [`named_file`]'s rule, for the variables that carry no
/// path. Presence alone would make `BT_PERF_TRACE=` mean *on*, and a shell that
/// wrote that meant the opposite — with a per-frame trace that dominates the
/// very profile it was set to read.
#[must_use]
pub fn switched_on(value: Option<OsString>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

/// **Did this run name the console as the place diagnostics go?**
///
/// The whole family of trace switches and nothing else: a variable whose name
/// begins `BT_` and mentions `TRACE`. Stated as a shape rather than a list
/// because a list is what the next trace variable is left off, and the failure
/// mode of being left off is silent — the trace is written and lands in a file
/// the developer is not watching.
///
/// Takes the environment as an iterator so this is a decision a test can make
/// without touching the process's own.
pub fn console_was_asked_for<I: IntoIterator<Item = OsString>>(names: I) -> bool {
    names.into_iter().any(|name| {
        let name = name.to_string_lossy().to_ascii_uppercase();
        name.starts_with("BT_") && name.contains("TRACE")
    })
}

/// Move `log` aside if it has grown past `cap`, keeping exactly one generation.
///
/// Answers whether a rotation happened. A log that cannot be moved is left
/// where it is and appended to: a diagnostic that refused to write because it
/// could not tidy up would be a diagnostic that fails hardest on the machines
/// that need it most.
pub fn rotate_if_oversized(log: &Path, previous: &Path, cap: u64) -> bool {
    let Ok(metadata) = std::fs::metadata(log) else {
        return false;
    };
    if metadata.len() < cap {
        return false;
    }
    // `rename` over an existing file is the replacement on Windows only if the
    // destination is gone first, which is why the previous generation is
    // removed rather than overwritten.
    let _ = std::fs::remove_file(previous);
    std::fs::rename(log, previous).is_ok()
}

/// The log this run writes to, under the storage directory.
#[must_use]
pub fn log_path(storage: &Path) -> PathBuf {
    storage.join(LOG_FILENAME)
}

/// **The front door closes here.**
///
/// Called once, from `main`, after the command line has been answered and
/// before the event loop is built — which is the whole of the ordering that
/// matters. Everything before this call reaches the console that started the
/// process; everything after it reaches the file, and the process stops being
/// a member of that console's group.
///
/// Answers which channel the rest of the run has, which is worth one line in a
/// startup trace and nothing else.
pub fn enter_resident_run(storage: &Path) -> Channel {
    if console_was_asked_for(std::env::vars_os().map(|(name, _)| name)) {
        // The console was named by this run. Keep it, keep the group membership
        // that comes with it, and rely on the control handler installed at the
        // front door for the `Ctrl+C` that membership exposes.
        return Channel::Console;
    }
    let log = log_path(storage);
    // The directory is the one `%APPDATA%\Folio\` that everything else in this
    // product already writes into; creating it here costs one call on a path
    // that almost always exists.
    let _ = std::fs::create_dir_all(storage);
    rotate_if_oversized(&log, &storage.join(PREVIOUS_LOG_FILENAME), LOG_ROTATE_AT);
    let channel = if bt_platform::redirect_std_streams_to_file(&log) {
        Channel::Log
    } else {
        bt_platform::silence_std_streams();
        Channel::Nowhere
    };
    // **After the streams have somewhere else to be**, so that nothing written
    // between the two calls could still reach the console.
    bt_platform::detach_console();
    channel
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{
        Channel, LOG_ROTATE_AT, console_was_asked_for, named_file, rotate_if_oversized, switched_on,
    };

    fn names(list: &[&str]) -> Vec<OsString> {
        list.iter().map(|name| OsString::from(*name)).collect()
    }

    /// PIN (user report, 2026-08-25: `Folio stopped: read BT_PROBE_INPUT : The
    /// system cannot find the path specified. (os error 3)`) — **an emptied
    /// variable is off, and never a file named the empty string.**
    ///
    /// Red gate: hand the empty string on as a path and the program dies at
    /// startup on a variable its owner believed they had switched off.
    #[test]
    fn an_emptied_variable_names_no_file() {
        assert_eq!(named_file(None), None);
        assert_eq!(named_file(Some(OsString::new())), None);
        assert_eq!(
            named_file(Some(OsString::from("probe.vt"))),
            Some(PathBuf::from("probe.vt")),
            "a variable that names something still names it"
        );
        assert_eq!(
            named_file(Some(OsString::from(" "))),
            Some(PathBuf::from(" ")),
            "and whitespace is a filename, not an emptiness this program \
             decides to see through"
        );
    }

    /// PIN — **the same word for the switches that carry no path.** Set-but-empty
    /// is off; a value of any kind is on.
    #[test]
    fn an_emptied_switch_is_off() {
        assert!(!switched_on(None));
        assert!(!switched_on(Some(OsString::new())));
        assert!(switched_on(Some(OsString::from("1"))));
        assert!(switched_on(Some(OsString::from("0"))), "any value is on");
    }

    /// PIN (console channel, 2026-08-25) — **the trace family keeps the console
    /// and nothing else does.**
    ///
    /// The two halves are two different faults. Forgetting a trace variable
    /// sends a developer's trace to a file they are not watching, silently.
    /// Admitting one that is not a trace — `BT_PTY_DUMP` above all, which the
    /// project's own test windows *always* carry — puts every resident
    /// diagnostic back on the pane that launched Folio, which is the fault this
    /// whole slice exists to end.
    #[test]
    fn the_console_is_kept_for_the_trace_family_and_for_nothing_else() {
        for asked in [
            "BT_STARTUP_TRACE",
            "BT_MOUSE_TRACE",
            "BT_MOUSE_TRACE_V",
            "BT_WEB_TRACE_V",
            "BT_ATTENTION_TRACE",
            "BT_PREVIEW_TRACE",
        ] {
            assert!(
                console_was_asked_for(names(&["PATH", asked, "APPDATA"])),
                "{asked} is a request for output on the shell that set it"
            );
        }
        assert!(
            !console_was_asked_for(names(&["PATH", "BT_PTY_DUMP", "BT_HANG_SELFTEST"])),
            "a dump that names its own file, and a switch that wedges the window \
             thread, ask for nothing on anybody's screen"
        );
        assert!(
            !console_was_asked_for(names(&["PATH", "APPDATA", "TRACE_ME", "TERM"])),
            "and the family is `BT_` first — a variable somebody else's tooling \
             set is not this product's instruction"
        );
        assert!(!console_was_asked_for(names(&[])));
    }

    /// PIN — **the log is bounded to two generations, and the rotation happens
    /// once, at the size it says.**
    ///
    /// Red gate: drop the cap check and a machine that logs a failure every
    /// frame fills a disk one line at a time, in a directory the user never
    /// opens.
    #[test]
    fn an_oversized_log_is_moved_aside_exactly_once() {
        let directory = std::env::temp_dir().join(format!(
            "folio-diagnostics-rotate-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a private directory for this test");
        let log = directory.join("diagnostics.log");
        let previous = directory.join("diagnostics.prev.log");

        assert!(
            !rotate_if_oversized(&log, &previous, 16),
            "a log that does not exist yet is not rotated"
        );
        std::fs::write(&log, "small").expect("a small log");
        assert!(
            !rotate_if_oversized(&log, &previous, 16),
            "and neither is one under the cap"
        );
        assert!(!previous.exists());

        std::fs::write(&log, vec![b'x'; 32]).expect("an oversized log");
        assert!(rotate_if_oversized(&log, &previous, 16));
        assert!(!log.exists(), "the oversized log is moved, not copied");
        assert_eq!(
            std::fs::metadata(&previous)
                .expect("the kept generation")
                .len(),
            32
        );

        // A second rotation replaces the kept generation rather than growing a
        // third: two files is the whole of the promise.
        std::fs::write(&log, vec![b'y'; 64]).expect("a second oversized log");
        assert!(rotate_if_oversized(&log, &previous, 16));
        assert_eq!(
            std::fs::read(&previous).expect("the kept generation").len(),
            64
        );
        let left: Vec<String> = std::fs::read_dir(&directory)
            .expect("read the directory back")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["diagnostics.prev.log"]);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// PIN — **`Nowhere` is a channel and it is not the console.** The enum is
    /// the place the policy is written down, and the failure this forecloses is
    /// a fourth arm that says "keep what we had".
    #[test]
    fn the_only_three_places_a_diagnostic_can_go_are_named() {
        assert_eq!(
            Channel::Nowhere.label(),
            "nowhere — its log could not be opened"
        );
        assert_ne!(Channel::Nowhere, Channel::Console);
        assert_eq!(LOG_ROTATE_AT, 4 * 1024 * 1024);
    }
}

/// **`docs/BT-ENVIRONMENT.md` is the list, and this is what keeps it the list.**
///
/// Several `BT_*` switches write terminal content to a path the person running
/// the program names, and a public build owes a complete account of them. A
/// document is only an account while it is complete, and the way documents stop
/// being complete is that somebody adds a switch. So the document is compared
/// against the source, in both directions, on every run of the tests.
#[cfg(test)]
mod bt_environment_doc_tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// The document, read at compile time so a missing file is a build failure
    /// rather than a skipped test.
    const DOCUMENT: &str = include_str!("../../../docs/BT-ENVIRONMENT.md");

    /// The repository root, from where this crate is rather than from where the
    /// test happened to be started.
    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .expect("the repository root, two directories above this crate")
    }

    /// Every `.rs` file that can end up in `folio.exe`.
    ///
    /// `src/bin/` is left out because those are development binaries that no
    /// release archive carries, and `tests/` because an integration test is not
    /// the shipped program. Everything else under `crates/` and `vendor/` is
    /// walked — **the walk is the point**: a list of files here would be a list
    /// somebody has to remember to add to, which is the same failure as a list
    /// of variables.
    fn shipped_sources(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for top in ["crates", "vendor"] {
            walk(&root.join(top), &mut found);
        }
        found.sort();
        assert!(
            found.len() > 50,
            "the walk found {} files, which is not a source tree",
            found.len()
        );
        found
    }

    fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name != "bin" && name != "tests" && name != "target" {
                    walk(&path, found);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    /// Every `BT_…` name that appears in `text` as a **whole** string literal —
    /// an opening quote, the name, a closing quote and nothing between.
    ///
    /// Whole rather than "contains", because that is exactly the line between a
    /// name and a sentence that begins with one: `"BT_PERSIST moved {} to {}"`
    /// is a diagnostic line and `"BT_PTY_DUMP"` is a variable, and no rule that
    /// looked only at the prefix could tell them apart. A name is at least one
    /// character past the underscore, so `starts_with("BT_")`'s own argument is
    /// not a name either.
    fn names_in_source(text: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        for (index, _) in text.match_indices("\"BT_") {
            let rest = &text[index + 1..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            if name.len() > 3 && rest[name.len()..].starts_with('"') {
                found.insert(name);
            }
        }
        found
    }

    /// Every `BT_…` name the document spells as a code span of its own.
    ///
    /// A span has to be the whole name and nothing else, so a header line or a
    /// shell fragment quoted in passing is prose rather than an entry.
    fn names_in_document(text: &str) -> BTreeSet<String> {
        text.split('`')
            .skip(1)
            .step_by(2)
            .filter(|span| {
                span.len() > 3
                    && span.starts_with("BT_")
                    && span
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            })
            .map(ToOwned::to_owned)
            .collect()
    }

    /// RED — **the document names every `BT_` the source does, and no others.**
    ///
    /// Release plan gate 4. RED GATE: add a whole `BT_ANYTHING` string literal
    /// to any shipped source and this fails naming it; delete an entry from the
    /// document and it fails the other way.
    #[test]
    fn every_bt_name_in_the_source_is_in_the_document_and_the_reverse() {
        let root = repository_root();
        let mut in_source = BTreeSet::new();
        for file in shipped_sources(&root) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            in_source.extend(names_in_source(&text));
        }
        let in_document = names_in_document(DOCUMENT);
        let undocumented: Vec<&String> = in_source.difference(&in_document).collect();
        assert!(
            undocumented.is_empty(),
            "these names are in the source and not in docs/BT-ENVIRONMENT.md: {undocumented:?}"
        );
        let stale: Vec<&String> = in_document.difference(&in_source).collect();
        assert!(
            stale.is_empty(),
            "these names are in docs/BT-ENVIRONMENT.md and not in the source: {stale:?}"
        );
    }

    /// The extractors themselves, because a gate that silently matched nothing
    /// would pass for ever.
    #[test]
    fn a_name_is_a_whole_literal_and_a_sentence_that_starts_with_one_is_not() {
        let source = names_in_source(
            "let a = \"BT_PTY_DUMP\"; eprintln!(\"BT_PERSIST moved {}\"); \
             name.starts_with(\"BT_\"); let b = \"BT_WEB_TRACE_V\";",
        );
        assert_eq!(
            source,
            ["BT_PTY_DUMP".to_owned(), "BT_WEB_TRACE_V".to_owned()]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        let document = names_in_document(
            "a `BT_PTY_DUMP` row, a `BT_MOUSE_TRACE_V1 elapsed_ms` header, `BT_`",
        );
        assert_eq!(
            document,
            ["BT_PTY_DUMP".to_owned()]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
    }

    /// The document is the one the release links to, so the two paths it promises
    /// are spelled in it.
    #[test]
    fn the_document_names_both_directories_the_product_writes_under() {
        assert!(DOCUMENT.contains(r"%APPDATA%\Folio"));
        assert!(DOCUMENT.contains(r"%LOCALAPPDATA%\Folio\WebView2"));
    }
}
