//! **Where this process says things, and when that changes.**
//!
//! Presentation freshness uses `note` independently of trace routing and hangs.
//! Its fixed numeric line formats are documented in `docs/PRESENT-DIAGNOSTICS.md`.
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
use std::time::SystemTime;

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

/// **The line a run writes before its first diagnostic.**
///
/// This file is appended to across runs and kept across one rotation, so what
/// arrives attached to a bug report is a stack of runs with no seam between
/// them. The header is that seam, and it carries the two facts that make
/// everything under it usable: which build wrote the lines that follow
/// (`crate::version`, the same sentence `--version` prints and the panic log
/// opens with) and which process, for a machine that had two windows open.
///
/// Pure, and taking the clock rather than reading it, so the shape of the line
/// is a thing a test can state.
///
/// **It ends with whether this build may update itself** — `updater on` or
/// `updater off`, [`crate::update::eligible`] (0.4.6 ticket U-8). A build fact
/// like the banner beside it, and here rather than in the banner because the
/// banner is `--version`'s answer, which three release scripts compare byte for
/// byte; this line is what `smoke.ps1`'s last step already reads, and it is the
/// line a bug report arrives with.
#[must_use]
pub fn run_header(now: &str, process_id: u32) -> String {
    format!(
        "── {} — run started {now}, pid {process_id}, updater {} ──",
        crate::version::banner(),
        if crate::update::eligible() {
            "on"
        } else {
            "off"
        }
    )
}

/// **The last line of the file, and the one a hang is read against** (§7.35).
///
/// [`run_header`]'s pair, in the same rule and rails so that a reader scrolling
/// a `diagnostics.log` sees where one run ends and the next begins — and so that
/// the question "did this build get to the end of `main`?" is answered by the
/// file rather than by a debugger. That question is not hypothetical here: on a
/// clean Windows 11 the process sets its exit code and then never leaves, and
/// this line is the difference between a shut that hung inside Folio and one
/// that hung after Folio had finished (`docs/DESIGN.md` §7.35).
#[must_use]
pub fn run_footer(now: &str, code: i32) -> String {
    format!(
        "── {} — run ended {now}, exit {code} ──",
        crate::version::banner()
    )
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
    // **The log is opened and headed before the channel is chosen**, and that
    // order is the whole of who owns this file's first line. See
    // [`open_run_log`].
    let previous_run_last_wrote = open_run_log(
        storage,
        &crate::hang_watch::utc_timestamp(std::time::SystemTime::now()),
        std::process::id(),
    );
    // **After the header and not before it**, because [`note`]'s destination is
    // a property of the storage directory and not of which channel won — a run
    // that kept its console writes its watchdog lines to this file and nothing
    // else to it — and a `note` that reached the file before the header would
    // be the line a bug report opens with.
    let _ = RESIDENT_LOG.set(log_path(storage));
    let channel = choose_resident_channel(storage, previous_run_last_wrote);
    // Remembered, and this is not bookkeeping: after this call `stdout` is very
    // often *this product's own log file*, and a later caller who asks "is
    // there a screen I can write on" by looking at the handle would be told yes
    // by the file it is already writing to. The panic hook asks exactly that.
    let _ = RESIDENT_CHANNEL.set(channel);
    channel
}

/// **Open this run's `diagnostics.log` and write the line that says whose it
/// is** — the one owner of that file's first line.
///
/// # Why this is a step of its own
///
/// The header used to be written from inside [`choose_resident_channel`], by
/// `eprintln!`, in the arm that had just pointed `stderr` at the log. Which
/// meant it was written for exactly one of the three channels: a run that kept
/// its console ([`Channel::Console`]) and a run whose log would not open
/// ([`Channel::Nowhere`]) wrote no header at all — while [`note`] went on
/// appending to the same file, because `note` has a handle of its own and does
/// not care where the streams point. So the first line of the file a bug report
/// arrives as was whatever the first watchdog line happened to be
/// (`Folio: the window thread held control for …`), with nothing above it
/// saying which build wrote it, and `smoke.ps1`'s last step read that and threw
/// (release of 0.4.2, 2026-09-18).
///
/// Written here, it is written by the step that *opens* the file, for every
/// kind of run, before [`RESIDENT_LOG`] exists for anything else to write
/// through — so "the first line of this run's block is this run's header" is
/// true by construction rather than by every later writer remembering it.
/// Through [`append_note`] and not `eprintln!` for the same reason: the header
/// belongs to the file, not to whichever stream this run ends up with.
///
/// Rotation comes with it, and moves with it for the same reason: a console run
/// writes to this log too, and on main it was the one kind of run that never
/// checked the cap.
///
/// Answers **when the previous run last wrote**, read before this run touches
/// the file, because that is the moment
/// [`report_the_previous_runs_crash`] measures a system crash report against —
/// and one line further down the header moves the timestamp past every report
/// there will ever be.
///
/// Takes the clock and the process id rather than reading them, so a test can
/// state the whole of the line it expects.
pub fn open_run_log(storage: &Path, now: &str, process_id: u32) -> Option<SystemTime> {
    // The directory is the one `%APPDATA%\Folio\` everything else in this
    // product already writes into; creating it here costs one call on a path
    // that almost always exists and is what makes the header land somewhere on
    // the first launch of all.
    let _ = std::fs::create_dir_all(storage);
    let log = log_path(storage);
    let previous_run_last_wrote = last_written(&log);
    rotate_if_oversized(&log, &storage.join(PREVIOUS_LOG_FILENAME), LOG_ROTATE_AT);
    append_note(&log, &run_header(now, process_id));
    previous_run_last_wrote
}

/// **One resident diagnostic, written where nothing can be waiting for a
/// console** (X-7).
///
/// # Why there is a second road to the same file
///
/// The channel this module installs is `SetStdHandle` and `dup2` precisely so
/// that no call site has to know where diagnostics go, and for the two hundred
/// and forty `eprintln!` in this workspace that is still the answer. But a run
/// that kept its console (a trace run — [`console_was_asked_for`]) writes those
/// lines into a pipe **somebody else is supposed to be reading**, and a reader
/// that stops reading stops the writer inside the kernel. Every thread that
/// then says anything queues behind it, because `eprintln!` is one
/// process-wide lock in front of one handle.
///
/// The two callers that must never queue there are the hang watchdog — whose
/// whole job is to be the thread still working when the window thread is not —
/// and the resident UI diagnostics the window thread itself writes. They come
/// here instead: **a handle of this function's own, opened for this line and
/// closed after it**, so there is no lock between two callers either, and a
/// `diagnostics.log` that is a file on the disk rather than somebody's screen.
///
/// # And the console still gets the line when somebody asked for one
///
/// Through the trace sink's bounded queue, which drops rather than waits — see
/// [`crate::trace_sink::offer_stderr_line`]. A developer watching a trace goes
/// on seeing the watchdog's lines; a developer whose shell has stopped reading
/// loses some of them and holds nobody up, which is the bargain every line in
/// that module is written under.
///
/// Before [`enter_resident_run`] there is no resident log and this says nothing:
/// the front door's output is [`eprintln!`]'s business and always was.
pub fn note(text: &str) {
    if let Some(log) = RESIDENT_LOG.get() {
        append_note(log, text);
    }
    if resident_channel() == Some(Channel::Console) {
        crate::trace_sink::offer_stderr_line(text.to_owned());
    }
}

/// [`note`]'s file half, taking the path so a test can drive it.
///
/// Opened, appended to and closed per line. That is more calls than a kept
/// handle would make and it is the point: a kept handle is a lock, and a lock
/// is a thread waiting for another thread's stalled write, which is the thing
/// this road exists to have none of. One `write_all` per line, to a handle the
/// platform opened in append mode, so two writers of this file interleave whole
/// lines rather than halves.
///
/// Answers whether the line reached the file.
pub fn append_note(log: &Path, text: &str) -> bool {
    let mut line = String::with_capacity(text.len() + 1);
    line.push_str(text);
    line.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()))
        .is_ok()
}

/// **This run's `diagnostics.log`**, remembered for [`note`] whichever channel
/// the run took — including a console run, where the streams never went near it
/// and the watchdog's lines are the only thing in it.
static RESIDENT_LOG: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// **Where the rest of this run's diagnostics can be found**, or `None` while
/// the front door is still open.
///
/// A `OnceLock` and not a parameter, because the one caller that needs it is a
/// panic hook: it can fire on any thread at any moment, including before the
/// front door has closed, and there is nothing to thread the answer through.
#[must_use]
pub fn resident_channel() -> Option<Channel> {
    RESIDENT_CHANNEL.get().copied()
}

static RESIDENT_CHANNEL: std::sync::OnceLock<Channel> = std::sync::OnceLock::new();

fn choose_resident_channel(storage: &Path, previous_run_last_wrote: Option<SystemTime>) -> Channel {
    if console_was_asked_for(std::env::vars_os().map(|(name, _)| name)) {
        // The console was named by this run. Keep it, keep the group membership
        // that comes with it, and rely on the control handler installed at the
        // front door for the `Ctrl+C` that membership exposes.
        return Channel::Console;
    }
    let log = log_path(storage);
    let channel = if bt_platform::redirect_std_streams_to_file(&log) {
        Channel::Log
    } else {
        bt_platform::silence_std_streams();
        Channel::Nowhere
    };
    // **After the streams have somewhere else to be**, so that nothing written
    // between the two calls could still reach the console.
    bt_platform::detach_console();
    if channel == Channel::Log {
        // **Under this run's header and not above it**, so that a reader
        // scrolling the file finds the news about the previous run inside the
        // run that noticed it, next to the build stamp that says which Folio
        // was doing the noticing. [`open_run_log`] wrote that header a moment
        // ago, whichever channel this turned out to be; this line is the one
        // that still depends on there being a stream pointed at the file.
        report_the_previous_runs_crash(previous_run_last_wrote);
    }
    channel
}

/// When a file was last written, or `None` for one that is not there — the
/// first launch ever, or a storage directory somebody emptied.
fn last_written(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// **Name the crash report the system wrote for the previous run, if it wrote
/// one** (M4-11, `docs/DESIGN.md` §13.31).
///
/// # What this is, and what it deliberately is not
///
/// On macOS a process that dies of a signal is not silent: `ReportCrash` writes
/// a complete `.ips` — every thread's backtrace, the register file, the loaded
/// images — into `~/Library/Logs/DiagnosticReports/`. Nobody who is not looking
/// for it will ever find it, which is the whole of the gap this closes: the
/// next launch looks, and says the path.
///
/// **It names and it does not copy**, and that is the Windows arm's behaviour
/// rather than a shortcut. Nothing in this product has ever written a crash
/// dump of its own — the minidumps §7.1.3u is argued from were taken by hand
/// with a debugger — and what the previous run's fate costs the *user* there is
/// one line: `BT_PERSIST previous session did not reach its clean-exit path`,
/// from [`crate::persist`], off the sentinel. This is that sentence with the
/// evidence's address in it. Copying the report beside the log would make a
/// second copy of a file the system already keeps, already rotates, and already
/// opens in Console.app when a reader double-clicks the path.
///
/// `None` — no log to measure against — reports nothing, because "newer than
/// the last thing the previous run said" has no meaning before there was a
/// previous run.
fn report_the_previous_runs_crash(since: Option<SystemTime>) {
    let Some(since) = since else {
        return;
    };
    let Some(directory) = bt_platform::hang::system_crash_reports_directory() else {
        return;
    };
    let Some(program) = this_programs_name() else {
        return;
    };
    let Some(report) = newest_crash_report(&directory, &program, since) else {
        return;
    };
    eprintln!(
        "Folio: the previous run ended in a crash the system recorded — {}",
        report.display()
    );
}

/// **The name the system files this process's crash reports under**: the
/// executable's own, which is what `ReportCrash` puts at the front of every
/// `.ips` file name.
///
/// Read off `current_exe` rather than written down as `folio`, and that is
/// generality and not caution: the shipped binary is `folio`, a development
/// build is `bt-app`, and a report is named after whichever one died. A
/// hard-coded product name would make this facility work for exactly the builds
/// nobody debugs.
fn this_programs_name() -> Option<String> {
    Some(
        std::env::current_exe()
            .ok()?
            .file_stem()?
            .to_string_lossy()
            .into_owned(),
    )
}

/// **The newest crash report in `directory` that names `program` and was
/// written after `since`.**
///
/// A function over a directory listing, so that the rule — which names count
/// and which moment they are measured against — is a thing a test can drive
/// with a temporary directory on either platform, rather than something only a
/// crashed Mac can demonstrate.
///
/// Unreadable entries are skipped rather than failing the walk: a
/// `DiagnosticReports` directory holds reports for every program this account
/// runs, and one of somebody else's that cannot be stat'ed is not a reason to
/// say nothing about ours.
#[must_use]
pub fn newest_crash_report(directory: &Path, program: &str, since: SystemTime) -> Option<PathBuf> {
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        if !names_a_crash_report(&entry.file_name().to_string_lossy(), program) {
            continue;
        }
        let Ok(written) = entry.metadata().and_then(|metadata| metadata.modified()) else {
            continue;
        };
        if written <= since {
            continue;
        }
        if newest.as_ref().is_none_or(|(best, _)| written > *best) {
            newest = Some((written, entry.path()));
        }
    }
    newest.map(|(_, path)| path)
}

/// **Is this file name a crash report for `program`?**
///
/// The system's own naming, and the whole of it: the process name, a separator,
/// something that says when, and `.ips` — `folio-2026-09-12-143022.ips` — or the
/// `.crash` the same directory held before macOS 12 and still can.
///
/// **The separator is the rule and not a nicety.** A bare prefix test would
/// claim `folioscope-2026-….ips` as Folio's, and a reader told that this
/// program crashed when another one did is worse served than one told nothing.
/// So the character after the name must be one the system uses to join the
/// fields — anything that is not a letter or a digit — or there must be no
/// character at all.
///
/// Case-insensitive, because the executable inside a bundle and the name a
/// report is filed under have disagreed about capitals before and neither
/// spelling is wrong.
#[must_use]
pub fn names_a_crash_report(file_name: &str, program: &str) -> bool {
    if program.is_empty() {
        return false;
    }
    let name = file_name.to_lowercase();
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    if !matches!(extension, "ips" | "crash") {
        return false;
    }
    let program = program.to_lowercase();
    let Some(rest) = stem.strip_prefix(&program) else {
        return false;
    };
    rest.chars()
        .next()
        .is_none_or(|next| !next.is_alphanumeric())
}

/// **Is there a screen a fault could be printed on?**
///
/// Asked by the panic hook, which has one decision to make: say it in text, or
/// raise a box. Two of the three resident channels answer no, and the reason is
/// the same for both — `stdout` no longer goes anywhere a person is looking.
/// Writing a crash into `diagnostics.log` is writing it into the file that is
/// already being written; writing it nowhere is worse. In both cases the box is
/// the only thing that reaches anybody.
///
/// `None` — the front door has not closed — is **yes**, because up there
/// `stdout` is still whatever the caller gave this process: their console,
/// their redirection, or nothing at all, and the write itself reports which.
#[must_use]
pub fn a_screen_is_watching(channel: Option<Channel>) -> bool {
    !matches!(channel, Some(Channel::Log | Channel::Nowhere))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{
        Channel, LOG_ROTATE_AT, append_note, console_was_asked_for, named_file,
        names_a_crash_report, newest_crash_report, rotate_if_oversized, switched_on,
    };

    fn names(list: &[&str]) -> Vec<OsString> {
        list.iter().map(|name| OsString::from(*name)).collect()
    }

    /// This file's own text, for the two source pins below.
    const DIAGNOSTICS: &str = include_str!("diagnostics.rs");

    /// The channel chooser's declaration, at column zero. Named once so that the
    /// two pins that read its body cannot come to mean two different functions.
    const CHOOSER: &str = "\nfn choose_resident_channel(storage: &Path, previous_run_last_wrote: Option<SystemTime>) -> Channel {";

    /// The text of the free function `opener` opens, to the `}` in column zero
    /// that closes it. The caller and nothing else — a whole-file search would
    /// answer with this crate's own tests, which quote these names to pin the
    /// order they are called in.
    fn body(source: &str, opener: &str) -> String {
        let at = source
            .find(opener)
            .unwrap_or_else(|| panic!("`{opener}` is declared once, at column zero"));
        let rest = &source[at..];
        let end = rest
            .find("\n}\n")
            .expect("a free function is closed by a `}` at column zero");
        rest[..end].to_owned()
    }

    /// RED (D3, the 0.4.2 release of 2026-09-18) — **the first line of a run's
    /// block in `diagnostics.log` is that run's header, and it is written by the
    /// step that opens the file rather than by the arm that happens to point a
    /// stream at it.**
    ///
    /// The file a bug report arrives as opened with
    /// `Folio: the window thread held control for …` and carried no build line
    /// at all, so nothing in it said which Folio wrote it and `smoke.ps1`'s last
    /// step threw on an otherwise green release. The cause is the ownership:
    /// [`super::note`] appends through a handle of its own, from the moment
    /// `RESIDENT_LOG` is set, while the header was written from inside
    /// `choose_resident_channel`'s `if channel == Channel::Log` arm — so a run
    /// that kept its console, or whose log would not take the streams, wrote
    /// notes into a file it had never headed.
    ///
    /// **RED ON MAIN:** the second assertion — that the channel chooser writes
    /// no header — is false on `main`, where `run_header` is called from inside
    /// that arm. The behavioural half pins the repair: the header is line 1 and
    /// the watchdog's line is line 2, for a run of any channel, because the
    /// header is written before `RESIDENT_LOG` exists for anything else to write
    /// through.
    ///
    /// MUTATION: move the `append_note` of the header back below the
    /// `RESIDENT_LOG.set` in `enter_resident_run` and the ordering this states
    /// stops being guaranteed; drop it and the first assertion goes red.
    #[test]
    fn a_run_heads_its_log_before_anything_else_can_write_into_it() {
        let storage = a_reports_directory("run-log");
        let log = super::log_path(&storage);

        let previous = super::open_run_log(&storage, "2026-09-20T01:02:03.456Z", 4242);
        assert_eq!(
            previous, None,
            "the first launch of all has no previous run to measure a crash report against"
        );

        // What an ordinary run says next, down the road that owes nobody a lock
        // — the watchdog's line, which is the one that was arriving as line 1.
        assert!(append_note(
            &log,
            "Folio: the window thread held control for 3971 ms on turn 15341"
        ));

        let text = std::fs::read_to_string(&log).expect("the log was made");
        let mut lines = text.lines();
        let header = lines.next().expect("the log is not empty");
        assert!(
            header.contains("run started")
                && header.contains(&crate::version::banner())
                && header.contains("pid 4242")
                && header.contains("2026-09-20T01:02:03.456Z"),
            "diagnostics.log opens with `{header}`, and that first line is what \
             smoke.ps1's last step reads and what a bug report is dated by"
        );
        assert_eq!(
            lines.next(),
            Some("Folio: the window thread held control for 3971 ms on turn 15341"),
            "and everything the run says afterwards is under its own header"
        );

        // A second run heads its own block, and reads the moment the first one
        // last wrote — before its own header moves that moment.
        let second = super::open_run_log(&storage, "2026-09-20T01:03:00.000Z", 4243);
        assert!(
            second.is_some(),
            "a run with a log already there measures against when it was last written"
        );
        let text = std::fs::read_to_string(&log).expect("the log is still there");
        assert_eq!(
            text.lines()
                .filter(|line| line.contains("run started"))
                .count(),
            2,
            "one header per run, and the file is the stack of them"
        );
        assert!(
            text.lines()
                .next_back()
                .is_some_and(|line| line.contains("pid 4243")),
            "the newest run's header is the last line of a file nothing else has written to yet"
        );

        // The source half: the header has one owner, and it is not the arm that
        // chose a stream. This is the assertion that is red on `main`.
        let chooser = body(DIAGNOSTICS, CHOOSER);
        assert!(
            !chooser.contains("run_header"),
            "the channel chooser writes the header again, so a run that kept its \
             console or could not open its log writes none:\n{chooser}"
        );
        assert!(
            body(DIAGNOSTICS, "\npub fn open_run_log(").contains("append_note(&log, &run_header("),
            "the header is no longer written by the step that opens the log"
        );

        let _ = std::fs::remove_dir_all(&storage);
    }

    /// RED (U-8) — **the first line of a run's block says whether the build
    /// that wrote it may update itself, and says it in the one of two words the
    /// build was made with.**
    ///
    /// This line is the self-report `smoke.ps1` already reads from an ordinary
    /// run, so it is where the release check asks the question without a second
    /// launch or a second window: a release must say `updater on`, and a
    /// development or CI build `updater off`. It is also what a bug report
    /// arrives with, and "could this copy have updated itself" is a question
    /// about which build it was, like the banner beside it.
    ///
    /// Through the real writer: the header is read back out of a log that
    /// [`super::open_run_log`] made, not assembled here.
    ///
    /// MUTATION: drop `updater {}` from `run_header`'s format, or print the
    /// words the other way round, and this goes red.
    #[test]
    fn the_run_header_says_whether_this_build_may_update_itself() {
        let storage = a_reports_directory("run-header-updater");
        let _ = super::open_run_log(&storage, "2026-09-26T01:02:03.456Z", 4244);
        let text = std::fs::read_to_string(super::log_path(&storage)).expect("the log was made");
        let header = text.lines().next().expect("the log is not empty");

        let (said, unsaid) = if crate::update::eligible() {
            ("updater on", "updater off")
        } else {
            ("updater off", "updater on")
        };
        assert!(
            header.contains(&format!(", {said} ──")),
            "diagnostics.log opens with `{header}`, which does not end by saying `{said}`"
        );
        assert!(
            !header.contains(unsaid),
            "and never the other word: `{header}`"
        );

        let _ = std::fs::remove_dir_all(&storage);
    }

    /// PIN — **the road that owes nobody a lock takes whole lines and makes its
    /// own file** (X-7).
    ///
    /// Two writers of one log, which is what a run with a `Log` channel actually
    /// has — the streams on one handle and this on another — and the thing that
    /// must survive it is a reader's ability to read a line. Appending, so a
    /// second note does not stand on the first.
    #[test]
    fn a_note_appends_a_whole_line_to_a_log_of_its_own() {
        let directory = a_reports_directory("notes");
        let log = directory.join(super::LOG_FILENAME);
        assert!(append_note(&log, "the window thread has not answered"));
        assert!(append_note(&log, "and then it did"));
        assert_eq!(
            std::fs::read_to_string(&log).expect("the log was made"),
            "the window thread has not answered\nand then it did\n"
        );
        assert!(
            !append_note(&directory, "a directory is not a log"),
            "a note that could not be written says so"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    // ── M4-11: the system's own crash reports ──────────────────────────────

    /// A directory of this test's own, under the machine's temporary folder.
    fn a_reports_directory(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let serial = COUNTER.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "bt-app-crash-reports-{tag}-{}-{serial}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        directory
    }

    /// Write a file and stamp it, so "newer than" is a fact the test states
    /// rather than one it hopes the filesystem's clock produced.
    fn a_report(directory: &std::path::Path, name: &str, at: std::time::SystemTime) {
        let path = directory.join(name);
        std::fs::write(&path, b"report").expect("the file is written");
        let file = std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("the file reopens");
        file.set_modified(at).expect("the stamp is set");
    }

    /// RED (M4-11, `docs/DESIGN.md` §13.31) — **a report is this program's only
    /// when the name is followed by a separator.**
    ///
    /// The system files a report as `<process>-<when>.ips`. A bare prefix test
    /// reads `folioscope-….ips` as Folio's, and a launch that told a reader
    /// "the previous run crashed" about another program's fault is worse than
    /// one that said nothing at all.
    ///
    /// MUTATION: drop the separator rule and the third assertion goes red; drop
    /// the extension rule and the log names whatever else is in that directory.
    #[test]
    fn a_crash_report_is_this_programs_only_when_the_name_is_followed_by_a_separator() {
        assert!(names_a_crash_report("folio-2026-09-12-143022.ips", "folio"));
        assert!(names_a_crash_report(
            "folio_2026-09-12-143022_mac-mini.crash",
            "folio"
        ));
        assert!(
            !names_a_crash_report("folioscope-2026-09-12-143022.ips", "folio"),
            "another program whose name starts the same way is not ours"
        );
        assert!(
            names_a_crash_report("bt-app-2026-09-12-143022.ips", "bt-app"),
            "a development build is named after the binary that died"
        );
        assert!(
            !names_a_crash_report("folio-2026-09-12-143022.diag", "folio"),
            "a power log in the same directory is not a crash report"
        );
        assert!(
            !names_a_crash_report("folio", "folio"),
            "and neither is a name with no extension at all"
        );
        assert!(
            names_a_crash_report("Folio-2026-09-12-143022.ips", "folio"),
            "the capitals of a bundle's executable are not a difference"
        );
        assert!(
            !names_a_crash_report("folio-2026.ips", ""),
            "a program with no name claims nothing"
        );
    }

    /// RED (M4-11) — **the newest report after the moment the previous run last
    /// wrote, and nothing from before it.**
    ///
    /// The moment is the log's own last write, so a report that has already been
    /// named by an earlier launch is behind it and is not named twice; a report
    /// from a crash that happened after that write is this crash.
    ///
    /// MUTATION: compare with `>=` against the wrong stamp, or take the first
    /// match instead of the newest, and one of the three assertions goes red.
    #[test]
    fn the_newest_report_after_the_previous_runs_last_word_is_the_one_named() {
        use std::time::{Duration, SystemTime};

        let directory = a_reports_directory("newest");
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_757_000_000);
        let since = base + Duration::from_secs(100);
        a_report(&directory, "folio-old.ips", base);
        a_report(&directory, "folio-new.ips", since + Duration::from_secs(10));
        a_report(
            &directory,
            "folio-newest.ips",
            since + Duration::from_secs(20),
        );
        a_report(
            &directory,
            "someoneelse-newest.ips",
            since + Duration::from_secs(30),
        );
        assert_eq!(
            newest_crash_report(&directory, "folio", since).and_then(|path| path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())),
            Some("folio-newest.ips".to_owned())
        );
        assert_eq!(
            newest_crash_report(&directory, "folio", since + Duration::from_secs(60)),
            None,
            "a launch that has already recorded them past this moment names none"
        );
        assert_eq!(
            newest_crash_report(&directory.join("not-there"), "folio", since),
            None,
            "a machine that files no reports is not an error"
        );
        std::fs::remove_dir_all(&directory).expect("the directory is removed");
    }

    /// RED — **the channel is chosen off the one door that reports whether it
    /// worked, and nothing is decided by the two that cannot fail** (M3-7,
    /// `docs/DESIGN.md` §13.23).
    ///
    /// The backend's five console doors split two ways off Windows and the split
    /// is this file's, not the backend's. `redirect_std_streams_to_file` is
    /// **load-bearing**: its `bool` is the whole of the difference between a run
    /// that has a log and one that says nothing, so it has a real arm on every
    /// platform. `adopt_parent_console` and `detach_console` are no-ops there —
    /// a Unix process is handed its parent's stdio by the kernel and joins no
    /// console group to leave — and the *reason that is allowed* is the claim
    /// below: their answers reach no branch, so an arm that does nothing is
    /// indistinguishable from one that did what there was to do.
    ///
    /// A source pin, because what is being held is the shape of two call sites
    /// rather than a value: `detach_console()` written as a statement, with no
    /// `if` and no `let` in front of it. The behavioural half is the backend's
    /// own — `bt_platform`'s `stream_tests` really moves this process's
    /// descriptors, on the platform where the arm compiles.
    ///
    /// MUTATIONS: wrap either call in an `if` and this goes red naming it — the
    /// launch would then take a different road on a platform whose answer is a
    /// constant. Drop the `if` in front of the redirect and the last assertion
    /// goes red: a run would claim `Channel::Log` without anything having asked
    /// whether the file opened.
    #[test]
    fn nothing_branches_on_the_two_console_no_ops() {
        const MAIN: &str = include_str!("main.rs");

        for (source, opener, where_it_is, call) in [
            (
                MAIN,
                "\nfn main() -> Result<()> {",
                "main.rs",
                "adopt_parent_console",
            ),
            (DIAGNOSTICS, CHOOSER, "diagnostics.rs", "detach_console"),
        ] {
            let caller = body(source, opener);
            let statement = format!("    bt_platform::{call}();\n");
            assert!(
                caller.contains(&statement),
                "`{call}` is not called for its effect alone in {where_it_is}; a \
                 platform whose answer to it is a constant would be taking a \
                 branch on that constant:\n{caller}"
            );
            assert_eq!(
                caller.matches(&format!("bt_platform::{call}")).count(),
                1,
                "`{call}` is named twice in that function in {where_it_is}, so \
                 one of the two is reading what it answers:\n{caller}"
            );
        }
        assert!(
            DIAGNOSTICS.contains("if bt_platform::redirect_std_streams_to_file(&log) {"),
            "the channel is no longer chosen off the door that reports whether \
             the log file took the streams, which is the one door here that can \
             fail on every platform"
        );
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
            "BT_CARD_TRACE",
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
