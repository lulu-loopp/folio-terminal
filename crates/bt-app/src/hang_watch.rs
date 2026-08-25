//! **A resident witness to the next hang: the window thread says where it is,
//! and a second thread writes down what it finds when it stops saying.**
//!
//! # Why this exists
//!
//! Two window-thread hangs have been found and fixed by hand — a `drain_leaf_pty`
//! whose only exit was "the ring is empty right now" (§1.3), and a PTY write with
//! no bound on either side (§1.3). Both were caught because somebody happened to
//! be at the machine with `hangprobe.ps1` ready. What is left is a white frame
//! and `Not Responding`, intermittently, on a build that has both fixes — which
//! means the remaining fault is something nobody has named, and the evidence for
//! it appears at a moment when nobody is watching.
//!
//! So the program watches itself. Not a debug switch: a switch that has to be
//! turned on before the bug is one that is off every time the bug happens.
//!
//! # The mechanism, in three sentences
//!
//! The window thread stamps a heartbeat once per turn of `about_to_wait`, and
//! leaves a **station** label — one byte, one store — at the door of each of the
//! long calls it makes. A watchdog thread wakes every two seconds and asks two
//! questions: has the turn counter moved, and if not, how long has it been. When
//! the answer crosses five seconds — thirty, before the loop has ever turned,
//! because a pump that has not started is not a pump that has stopped — it
//! suspends the window thread for exactly two
//! kernel calls, reads its registers and its raw stack (see
//! [`bt_platform::hang`]), resumes it, and writes what it found —
//! **the station, the silence, the stack, and the run's counters** — to a file
//! under `%APPDATA%\Folio\hang-reports\`.
//!
//! # It reports and it does not intervene
//!
//! Nothing here kills, restarts, unwedges, or shows the user a dialog. A
//! watchdog that acts is a watchdog that can be wrong about a process that was
//! merely slow, and the cost of being wrong is somebody's scrollback. The
//! window thread is resumed the instant the register file has been copied out,
//! whatever else went wrong; if the pump comes back afterwards, a second line is
//! appended to the same file saying how long it was gone. **A report with no
//! such line is a hang that never ended**, and that absence is itself the
//! finding.
//!
//! # What it costs when nothing is wrong
//!
//! This is a resident facility, so the bill has to be small enough that nobody
//! would think about turning it off:
//!
//! - **Per station**, [`at`] is one relaxed `u8` store to a static — on x86-64 a
//!   single `mov`, no fence, no branch, no clock read, no allocation. There are
//!   eight of them, at function entries, on a loop turn that already does far
//!   more work than that in its first line.
//! - **Per turn of the loop**, [`beat`] is one `Instant::now()` (which
//!   `about_to_wait` already calls for its own clocks) plus three stores.
//! - **Per two seconds, forever**, the watchdog does one `Instant::now()`, three
//!   atomic loads and a comparison, then sleeps again. **Zero allocation**: the
//!   idle path never touches the heap, never opens a file, and never creates the
//!   reports directory — a run that does not hang leaves nothing on the disk at
//!   all.
//! - **Per report**, and only then: a module enumeration, a 128 KiB read and a
//!   file write, all on the watchdog thread. Nothing is ever written from the
//!   window thread.
//!
//! The watchdog runs in the `BelowNormal` band with every other worker (§1.4).
//! That is the right band even though its job is to run when the window thread
//! cannot: the hangs in question are a thread that is *blocked*, not a machine
//! with no cores left, and a diagnostic that outranked the frame would be paying
//! for itself out of the thing it exists to protect.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How often the watchdog looks.
///
/// Two seconds: fast enough that a five-second threshold is crossed within
/// seven, slow enough that the whole idle cost of this facility is 30 atomic
/// loads a minute.
const WATCH_INTERVAL: Duration = Duration::from_secs(2);

/// How long the pump may be silent before it is a hang.
///
/// Five seconds because that is well past anything the loop legitimately does —
/// the longest measured single-turn cost in the perf-resilience work (§1.4) was
/// a 1.25 s frame under 24-way `cargo` — and it is also the neighbourhood where
/// Windows itself starts drawing the ghost window and saying `Not Responding`,
/// which is the symptom the user reports.
const HANG_THRESHOLD: Duration = Duration::from_secs(5);

/// How long the loop may take to reach its **first** turn before that, too, is a
/// hang.
///
/// Thirty seconds, and the number comes from the first real run of this
/// watchdog: a cold debug launch spent eight seconds building the event loop,
/// the GPU device and the first shell before `about_to_wait` ran once, and
/// filed a report at station `starting` for a program that was working. See the
/// branch in [`HangWatch::poll`]. Long enough that only a start that is truly
/// stuck reaches it; short enough that "I double-clicked it and nothing
/// happened" still produces a file.
const STARTUP_THRESHOLD: Duration = Duration::from_secs(30);

/// How many reports are kept. The oldest beyond this are deleted.
///
/// Sixteen: enough that a user who hits the fault four times in an evening
/// still has all four plus history, few enough that a pathological run cannot
/// fill a disk one 8 KiB file at a time.
const REPORTS_KEPT: usize = 16;

/// How many stack candidates one report prints.
const MAX_FRAMES: usize = 96;

/// The directory reports go in, under `%APPDATA%\Folio\`.
pub const REPORTS_DIRECTORY: &str = "hang-reports";

/// **Where the window thread was when it last said anything.**
///
/// One byte, and deliberately coarse. This is the *floor* of the evidence: when
/// the module map fails, or `GetThreadContext` is refused, or the stack scan
/// finds nothing but `ntdll`, this label still says which of the loop's long
/// calls the thread had entered — and for the three hangs this project has
/// actually had, that alone would have named the culprit.
///
/// Note what it means precisely: **the last station entered**, not the station
/// currently executing. A thread that entered `Drain`, returned from it, and
/// then wedged in some untagged code between stations still reports `Drain`.
/// That is honest — it is the last thing known — and it is why the stack is
/// captured too.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Station {
    /// Before the loop has taken its first turn.
    #[default]
    Starting = 0,
    /// The top of `about_to_wait`: the loop is going round.
    Wait = 1,
    /// Inside `window_event`: the platform handed us something.
    Event = 2,
    /// `Runtime::drain_pty` — every shell's output, one quantum each.
    Drain = 3,
    /// `Runtime::flush_pending_pty_resize` — the synchronous `ResizePseudoConsole`
    /// round trip into conhost, one per pane per quiet window.
    PtyResize = 4,
    /// `Runtime::publish_frame_inner` — compose, acquire, submit, present.
    Present = 5,
    /// `Runtime::flush_wheel` — a coalesced burst of notches being spent.
    Wheel = 6,
    /// `Runtime::advance_web_page` — a call into WebView2 and therefore into
    /// another process.
    WebPage = 7,
    /// `SessionStore::flush_if_due` — the autosave's own door.
    Autosave = 8,
    /// The deliberate hang of [`run_selftest_if_due`]. Debug builds only.
    SelfTest = 9,
}

impl Station {
    /// The word the report prints.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Wait => "about_to_wait",
            Self::Event => "window_event",
            Self::Drain => "drain_pty",
            Self::PtyResize => "flush_pending_pty_resize",
            Self::Present => "publish_frame_inner",
            Self::Wheel => "flush_wheel",
            Self::WebPage => "advance_web_page",
            Self::Autosave => "session flush_if_due",
            Self::SelfTest => "BT_HANG_SELFTEST",
        }
    }

    /// The inverse of the `repr`, for reading the atomic back.
    ///
    /// Total rather than fallible: the only writer is [`Heartbeat::at`], which
    /// only ever stores a value that came from this enum, so an unknown byte is
    /// unreachable — and a report is the wrong place to discover that by
    /// panicking.
    #[must_use]
    fn from_byte(byte: u8) -> Self {
        match byte {
            1 => Self::Wait,
            2 => Self::Event,
            3 => Self::Drain,
            4 => Self::PtyResize,
            5 => Self::Present,
            6 => Self::Wheel,
            7 => Self::WebPage,
            8 => Self::Autosave,
            9 => Self::SelfTest,
            _ => Self::Starting,
        }
    }
}

impl fmt::Display for Station {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// One reading of the window thread's pulse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pulse {
    /// Milliseconds from the heartbeat's origin to the loop's last turn.
    pub at_ms: u64,
    /// **How many turns the loop has taken.** Liveness is this number moving,
    /// not the clock — a stopped thread's `at_ms` is a perfectly stable value,
    /// and a loop that spins without progressing would still be reading the
    /// clock. The counter only advances where the pump actually comes round.
    pub turn: u64,
    pub station: Station,
}

/// The window thread's own three words, and the clock they are measured on.
///
/// Not a `Mutex` and not a channel: the writer is the thread this exists to
/// diagnose, so the write has to be something that cannot itself block, cannot
/// allocate, and cannot be what wedges. Three atomics is the whole of it.
#[derive(Debug)]
pub struct Heartbeat {
    origin: Instant,
    at_ms: AtomicU64,
    turn: AtomicU64,
    station: AtomicU8,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self::new()
    }
}

impl Heartbeat {
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            at_ms: AtomicU64::new(0),
            turn: AtomicU64::new(0),
            station: AtomicU8::new(Station::Starting as u8),
        }
    }

    /// Milliseconds since this heartbeat started. Monotonic.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// **The window thread came round.** Called once per `about_to_wait`.
    ///
    /// The turn counter is stored last and with `Release`, and read first and
    /// with `Acquire` in [`Self::sample`]. That pairing is what makes the
    /// sample coherent: a watchdog that observed a new turn is guaranteed to
    /// observe the timestamp that belongs to it, rather than the previous
    /// turn's — which, at a two-second poll against a five-second threshold,
    /// would be the difference between "quiet" and a report.
    pub fn beat(&self) {
        self.at_ms.store(self.now_ms(), Ordering::Relaxed);
        self.station.store(Station::Wait as u8, Ordering::Relaxed);
        self.turn.fetch_add(1, Ordering::Release);
    }

    /// **The window thread entered a named call.** One relaxed store.
    ///
    /// No ordering, because none is owed: this is a hint about a thread that is
    /// still running, and a watchdog that reads a station one instruction stale
    /// has read a true fact about one instruction ago.
    pub fn at(&self, station: Station) {
        self.station.store(station as u8, Ordering::Relaxed);
    }

    /// Read all three, coherently. Safe from any thread.
    #[must_use]
    pub fn sample(&self) -> Pulse {
        let turn = self.turn.load(Ordering::Acquire);
        Pulse {
            at_ms: self.at_ms.load(Ordering::Relaxed),
            turn,
            station: Station::from_byte(self.station.load(Ordering::Relaxed)),
        }
    }
}

/// The one heartbeat this process has.
///
/// A `LazyLock` rather than a `OnceLock` asked with `get_or_init` at every
/// station: the deref is a single acquire load of an already-initialised cell,
/// which is what makes [`at`] cheap enough to put at a function entry without
/// thinking about it. The origin instant is fixed at whichever of `main`'s
/// first two calls touches it, which is before the event loop is built.
static HEARTBEAT: LazyLock<Heartbeat> = LazyLock::new(Heartbeat::new);

/// The process's heartbeat.
#[must_use]
pub fn heartbeat() -> &'static Heartbeat {
    &HEARTBEAT
}

/// The window thread came round. See [`Heartbeat::beat`].
pub fn beat() {
    HEARTBEAT.beat();
}

/// The window thread entered `station`. See [`Heartbeat::at`].
pub fn at(station: Station) {
    HEARTBEAT.at(station);
}

/// What the watchdog decided on one look.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// The pump is running, or it is quiet but not yet for long enough.
    Quiet,
    /// The pump has just crossed the threshold. **Write a report.**
    Hung {
        silent_ms: u64,
        /// The threshold that was actually crossed, which is not always
        /// [`HANG_THRESHOLD`] — a loop that has not taken its first turn is
        /// judged against [`STARTUP_THRESHOLD`], and a report that named the
        /// wrong one would be a report that lies about its own trigger.
        threshold_ms: u64,
        station: Station,
        turn: u64,
    },
    /// Still stopped, and already reported. Say nothing more.
    StillHung { silent_ms: u64 },
    /// The pump came back after a report was written. **Append one line.**
    Healed { hung_ms: u64, station: Station },
}

/// The whole of the decision, with no clock, no thread and no disk in it.
///
/// Split out from the thread that drives it so that "a stalled pump produces
/// exactly one report and exactly one healing line" is a claim a unit test can
/// make by handing it numbers, rather than something a person has to reproduce
/// by wedging a terminal.
#[derive(Clone, Copy, Debug)]
pub struct HangWatch {
    threshold_ms: u64,
    /// The threshold that applies before the loop has taken its first turn. See
    /// the branch in [`HangWatch::poll`] that chooses between the two.
    startup_threshold_ms: u64,
    seen_turn: u64,
    /// Whether a turn has ever been observed. Latched, never cleared: a loop
    /// that has run once has proved it can, and the ordinary threshold is the
    /// right one from then on even if it later stops for good.
    started: bool,
    /// `Some` from the moment a report is written until the pump moves again.
    /// Carries the timestamp of the last turn *before* the stall, which is what
    /// the healing line measures from.
    stall_began_ms: Option<u64>,
    stall_station: Station,
}

impl HangWatch {
    #[must_use]
    pub fn new(threshold: Duration, startup_threshold: Duration) -> Self {
        Self {
            threshold_ms: u64::try_from(threshold.as_millis()).unwrap_or(u64::MAX),
            startup_threshold_ms: u64::try_from(startup_threshold.as_millis()).unwrap_or(u64::MAX),
            // The turn counter starts at zero and the first `beat` makes it one,
            // so zero here means "nothing has been seen yet" without needing a
            // sentinel.
            seen_turn: 0,
            started: false,
            stall_began_ms: None,
            stall_station: Station::Starting,
        }
    }

    /// One look. `now_ms` and `pulse` are read from the same clock.
    pub fn poll(&mut self, now_ms: u64, pulse: Pulse) -> Verdict {
        if pulse.turn != self.seen_turn {
            self.seen_turn = pulse.turn;
            self.started = true;
            if let Some(began) = self.stall_began_ms.take() {
                return Verdict::Healed {
                    // Measured between two stamps the *window thread* wrote —
                    // its last turn before the stall and its first turn after —
                    // rather than between the watchdog's two wakings, which
                    // would round the answer up by a poll interval.
                    hung_ms: pulse.at_ms.saturating_sub(began),
                    station: self.stall_station,
                };
            }
            return Verdict::Quiet;
        }
        let silent_ms = now_ms.saturating_sub(pulse.at_ms);
        if self.stall_began_ms.is_some() {
            return Verdict::StillHung { silent_ms };
        }
        // **A pump that has never run cannot be said to have stopped.** Until a
        // turn has actually been observed, all that is known is that the loop
        // has not started, which is a different fault on a different timescale:
        // building the event loop, the GPU device and the first shell is
        // legitimately slow — the first real run of this watchdog filed a report
        // at `starting` because a cold debug launch took eight seconds to reach
        // `about_to_wait`, and a diagnostic that files a report on every cold
        // boot is a diagnostic people learn to ignore.
        //
        // Not blindness: a start that *never* finishes is exactly the shape of
        // "I double-clicked it and nothing happened", so it is still reported —
        // just after a grace long enough that only a genuinely stuck start
        // reaches it. Once one turn has been seen, the ordinary threshold
        // applies for the rest of the run.
        let threshold_ms = if self.started {
            self.threshold_ms
        } else {
            self.startup_threshold_ms
        };
        if silent_ms < threshold_ms {
            return Verdict::Quiet;
        }
        self.stall_began_ms = Some(pulse.at_ms);
        self.stall_station = pulse.station;
        Verdict::Hung {
            silent_ms,
            threshold_ms,
            station: pulse.station,
            turn: pulse.turn,
        }
    }
}

/// Everything one report says, gathered before a word of it is formatted.
///
/// A struct rather than arguments so that [`render_report`] is a pure function
/// of facts and can be tested against a fixed sample — which matters, because
/// the one thing that must never be wrong about a hang report is the report.
pub struct ReportFacts<'a> {
    /// UTC, as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub written_at: &'a str,
    pub process_id: u32,
    pub ui_thread_id: u32,
    pub uptime_ms: u64,
    pub silent_ms: u64,
    pub threshold_ms: u64,
    pub station: Station,
    pub turn: u64,
    pub stack: &'a bt_platform::hang::StackSample,
    pub surfaces: bt_render::SurfaceFailureTally,
}

/// Milliseconds, as a number a person reads without counting zeroes.
fn seconds(milliseconds: u64) -> String {
    format!("{}.{:03}s", milliseconds / 1000, milliseconds % 1000)
}

/// The report, as text.
#[must_use]
pub fn render_report(facts: &ReportFacts<'_>) -> String {
    // Locally, because this module also writes to files and `std::io::Write`
    // and `std::fmt::Write` both offer `write_fmt`.
    use std::fmt::Write as _;

    let mut out = String::with_capacity(8192);
    out.push_str("Folio hang report\n");
    out.push_str("=================\n");
    out.push_str(
        "The window thread stopped turning its loop for longer than the threshold below.\n\
         It was suspended for two kernel calls to take this sample and resumed immediately;\n\
         nothing here killed, restarted or unwedged anything. If there is no `healed` line\n\
         at the end of this file, the pump never came back before the process ended.\n\n",
    );
    let _ = writeln!(out, "written        : {} (UTC)", facts.written_at);
    let _ = writeln!(
        out,
        "process        : pid {}, ui thread {}",
        facts.process_id, facts.ui_thread_id
    );
    let _ = writeln!(out, "uptime         : {}", seconds(facts.uptime_ms));
    let _ = writeln!(
        out,
        "pump silent for: {} (threshold {})",
        seconds(facts.silent_ms),
        seconds(facts.threshold_ms)
    );
    let _ = writeln!(
        out,
        "last station   : {} (the last one entered, not necessarily the one it is in)",
        facts.station
    );
    let _ = writeln!(out, "loop turns     : {}", facts.turn);

    out.push_str("\nui thread stack\n");
    let stack = facts.stack;
    if let Some(note) = stack.note {
        let _ = writeln!(out, "  note   : {note}");
    }
    let _ = writeln!(
        out,
        "  rip    : {}",
        stack.rip_site.as_ref().map_or_else(
            || format!("0x{:016x} (no module)", stack.rip),
            ToString::to_string
        )
    );
    let _ = writeln!(out, "  rsp    : 0x{:016x}", stack.rsp);
    let _ = writeln!(
        out,
        "  read   : {} bytes of stack, {} modules mapped",
        stack.scanned_bytes, stack.modules
    );
    if stack.frames.is_empty() {
        out.push_str("  (no module-resolvable addresses on the stack)\n");
    } else {
        out.push_str(
            "  candidate return addresses, innermost first. Unfiltered: an address that has\n\
             already returned stays on the stack until something overwrites it, so this list\n\
             over-reports on purpose rather than filtering away the one that mattered.\n",
        );
        for site in &stack.frames {
            let _ = writeln!(out, "  [+0x{:05x}] {site}", site.depth);
        }
    }

    out.push_str("\nrun counters\n");
    let tally = facts.surfaces;
    if tally.is_clean() {
        out.push_str("  surface acquires: clean — nothing has failed in this run\n");
    } else {
        let _ = writeln!(
            out,
            "  surface acquires: unavailable {}, outdated {}, lost {}, validation {} (total {})",
            tally.unavailable,
            tally.outdated,
            tally.lost,
            tally.validation,
            tally.total()
        );
    }
    out
}

/// The line appended when the pump comes back.
#[must_use]
pub fn render_healed(hung_ms: u64, station: Station) -> String {
    format!(
        "\nhealed         : the pump came back after {} at {}\n",
        seconds(hung_ms),
        station
    )
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` from a wall clock, with no date-time dependency.
///
/// The calendar is [`crate::seed::civil_from_days`], which the git panel already
/// shares for the same reason: one implementation of the Gregorian rules in this
/// workspace, not two that can disagree about a leap year.
#[must_use]
pub fn utc_timestamp(now: SystemTime) -> String {
    let since_epoch = now.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let seconds = since_epoch.as_secs();
    let millis = since_epoch.subsec_millis();
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let time_of_day = seconds % 86_400;
    let (year, month, day) = crate::seed::civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

/// The file one report is written to.
///
/// The timestamp is spelled so that **lexicographic order is chronological
/// order**, which is the whole of [`prune_reports`]'s sort: no `read_dir`
/// metadata call, no dependence on a filesystem's opinion of modification time,
/// and a name a person can read.
#[must_use]
pub fn report_filename(timestamp: &str) -> String {
    let compact: String = timestamp
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect();
    format!("hang-{compact}.txt")
}

/// Keep the newest `keep` reports and delete the rest.
///
/// Runs before each write, on the watchdog thread. Files that are not ours are
/// not touched — this directory is the product's, but a person who has dropped
/// a note in it should find the note still there.
pub fn prune_reports(directory: &Path, keep: usize) -> std::io::Result<usize> {
    let mut ours: Vec<PathBuf> = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("hang-") && name.ends_with(".txt"))
        })
        .collect();
    if ours.len() <= keep {
        return Ok(0);
    }
    ours.sort();
    let doomed = ours.len() - keep;
    let mut removed = 0;
    for path in ours.into_iter().take(doomed) {
        if fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// **Start watching.** Called once, from the window thread, before the loop.
///
/// `reports` is resolved by the caller rather than by this module so that the
/// `%APPDATA%` lookup — which is a `OnceLock` that also performs the one-time
/// `BetterTerminal` → `Folio` relocation — happens on the window thread at a
/// moment of the program's choosing, and so that a test can point it somewhere
/// private.
///
/// Failing to spawn is said out loud and dropped: a terminal that refused to
/// start because it could not arrange to diagnose itself would be a worse
/// program than one that starts without the diagnosis.
pub fn start(reports: PathBuf) {
    let ui_thread_id = bt_platform::hang::current_thread_id();
    // Touch the heartbeat here so its origin is the start of the run rather
    // than the first station, which makes `uptime` in a report mean what it
    // says.
    let _ = heartbeat().now_ms();
    if let Err(error) = bt_platform::spawn_at_priority(
        "bt-hang-watch",
        bt_platform::ThreadPriority::BelowNormal,
        move || watch_forever(reports, ui_thread_id),
    ) {
        eprintln!("Folio could not start its hang watchdog: {error}");
    }
}

/// The watchdog thread's whole life.
fn watch_forever(reports: PathBuf, ui_thread_id: u32) {
    let mut watch = HangWatch::new(HANG_THRESHOLD, STARTUP_THRESHOLD);
    // The file the stall in progress was reported to, so its healing line lands
    // in the same file rather than in a second one nobody would connect to it.
    let mut open_report: Option<PathBuf> = None;
    loop {
        std::thread::sleep(WATCH_INTERVAL);
        let heart = heartbeat();
        // Two atomic loads and a clock read. This is the entire steady-state
        // cost of the facility.
        match watch.poll(heart.now_ms(), heart.sample()) {
            Verdict::Quiet | Verdict::StillHung { .. } => {}
            Verdict::Hung {
                silent_ms,
                threshold_ms,
                station,
                turn,
            } => {
                open_report = write_report(
                    &reports,
                    ui_thread_id,
                    silent_ms,
                    threshold_ms,
                    station,
                    turn,
                    heart.now_ms(),
                );
            }
            Verdict::Healed { hung_ms, station } => {
                if let Some(path) = open_report.take() {
                    append_healed(&path, hung_ms, station);
                }
            }
        }
    }
}

/// Take the sample and put it on the disk. Answers where it landed.
fn write_report(
    reports: &Path,
    ui_thread_id: u32,
    silent_ms: u64,
    threshold_ms: u64,
    station: Station,
    turn: u64,
    uptime_ms: u64,
) -> Option<PathBuf> {
    // **The suspend happens here and nowhere else.** Everything above is
    // arithmetic; everything below is formatting.
    let stack = bt_platform::hang::capture_thread_stack(ui_thread_id, MAX_FRAMES);
    let timestamp = utc_timestamp(SystemTime::now());
    let facts = ReportFacts {
        written_at: &timestamp,
        process_id: std::process::id(),
        ui_thread_id,
        uptime_ms,
        silent_ms,
        threshold_ms,
        station,
        turn,
        stack: &stack,
        surfaces: bt_render::surface_failure_tally(),
    };
    let body = render_report(&facts);
    // Created lazily: a run that never hangs never makes this directory.
    if let Err(error) = fs::create_dir_all(reports) {
        eprintln!(
            "Folio saw its window thread stop for {} at {station} but could not create {}: {error}",
            seconds(silent_ms),
            reports.display()
        );
        return None;
    }
    let _ = prune_reports(reports, REPORTS_KEPT.saturating_sub(1));
    let path = reports.join(report_filename(&timestamp));
    match File::create(&path).and_then(|mut file| file.write_all(body.as_bytes())) {
        Ok(()) => {
            eprintln!(
                "Folio's window thread has not answered for {}; last station {station}. Report: {}",
                seconds(silent_ms),
                path.display()
            );
            Some(path)
        }
        Err(error) => {
            eprintln!("Folio could not write {}: {error}", path.display());
            None
        }
    }
}

fn append_healed(path: &Path, hung_ms: u64, station: Station) {
    let line = render_healed(hung_ms, station);
    if let Ok(mut file) = OpenOptions::new().append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// A deliberate hang, so that the reporter can be tested against a fault whose
/// answer is known.
///
/// **Debug builds only, and the release build does not read the variable at
/// all.** A shipped `folio.exe` that an environment variable can wedge for ten
/// seconds is a denial of service with a documentation page, and the thing this
/// verifies — that a stopped pump produces a file naming the line that stopped
/// it — is verified once, by a developer, on a debug build.
///
/// `BT_HANG_SELFTEST=<seconds>` holds the window thread for that many seconds,
/// **once**, on the first turn at least [`SELFTEST_ARM`] after the run started —
/// late enough that the window is on the glass and the watchdog is already
/// looking, so the report describes a real terminal and not a half-built one.
#[cfg(debug_assertions)]
const SELFTEST_ARM: Duration = Duration::from_secs(3);

/// Set once the deliberate hang has been performed, so it happens once.
#[cfg(debug_assertions)]
static SELFTEST_FIRED: AtomicBool = AtomicBool::new(false);

/// Parsed once. Reading the environment on every turn of the loop would be an
/// allocation and a lock per frame, which is a strange price for a diagnostic
/// that is off.
#[cfg(debug_assertions)]
static SELFTEST_HOLD: LazyLock<Option<Duration>> = LazyLock::new(|| {
    let seconds = std::env::var("BT_HANG_SELFTEST").ok()?;
    let seconds: u64 = seconds.trim().parse().ok().filter(|value| *value > 0)?;
    Some(Duration::from_secs(seconds))
});

/// Hold the window thread, once, if a debug build was asked to. See
/// [`SELFTEST_ARM`].
#[cfg(debug_assertions)]
pub fn run_selftest_if_due() {
    if SELFTEST_FIRED.load(Ordering::Relaxed) {
        return;
    }
    let Some(hold) = *SELFTEST_HOLD else {
        return;
    };
    let heart = heartbeat();
    if heart.now_ms() < u64::try_from(SELFTEST_ARM.as_millis()).unwrap_or(u64::MAX) {
        return;
    }
    if SELFTEST_FIRED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "BT_HANG_SELFTEST: holding the window thread for {}s on purpose",
        hold.as_secs()
    );
    at(Station::SelfTest);
    std::thread::sleep(hold);
}

/// Release builds do not read `BT_HANG_SELFTEST`. See the debug half.
#[cfg(not(debug_assertions))]
pub fn run_selftest_if_due() {}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{
        HangWatch, Heartbeat, Pulse, ReportFacts, Station, Verdict, prune_reports, render_healed,
        render_report, report_filename, utc_timestamp,
    };

    fn pulse(at_ms: u64, turn: u64, station: Station) -> Pulse {
        Pulse {
            at_ms,
            turn,
            station,
        }
    }

    /// PIN (hang reporter, 2026-08-25) — **a stopped pump produces exactly one
    /// report, and the healing line is measured between the window thread's own
    /// two stamps.**
    ///
    /// The three failures this pins, in the order they would happen: a watchdog
    /// that reported on every poll would write a file every two seconds for as
    /// long as the hang lasted; one that forgot it had reported would never
    /// write the healing line; one that measured the outage between its own
    /// wakings would round it up by a poll interval and say so in a file people
    /// will quote.
    #[test]
    fn a_stalled_pump_is_reported_once_and_its_recovery_is_accounted_for() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        // Three healthy turns.
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait)),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(3_000, pulse(2_900, 2, Station::Wait)),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(5_000, pulse(4_900, 3, Station::Drain)),
            Verdict::Quiet
        );
        // The pump stops at 4_900 with `Drain` as its last station.
        assert_eq!(
            watch.poll(7_000, pulse(4_900, 3, Station::Drain)),
            Verdict::Quiet,
            "2.1s of silence is not yet a hang"
        );
        assert_eq!(
            watch.poll(9_900, pulse(4_900, 3, Station::Drain)),
            Verdict::Hung {
                silent_ms: 5_000,
                threshold_ms: 5_000,
                station: Station::Drain,
                turn: 3,
            },
            "the threshold is crossed and the one report is owed here"
        );
        assert_eq!(
            watch.poll(11_900, pulse(4_900, 3, Station::Drain)),
            Verdict::StillHung { silent_ms: 7_000 },
            "and never a second report for the same stall"
        );
        assert_eq!(
            watch.poll(13_900, pulse(4_900, 3, Station::Drain)),
            Verdict::StillHung { silent_ms: 9_000 }
        );
        // The pump comes back: its next turn stamps 13_400.
        assert_eq!(
            watch.poll(15_900, pulse(13_400, 4, Station::Wait)),
            Verdict::Healed {
                hung_ms: 8_500,
                station: Station::Drain,
            },
            "13_400 - 4_900, both written by the window thread, and not 15_900 - 4_900"
        );
        assert_eq!(
            watch.poll(17_900, pulse(15_900, 5, Station::Wait)),
            Verdict::Quiet,
            "a recovered watch is armed again and silent again"
        );
    }

    /// PIN — **a second stall after a recovery is reported again.** The state
    /// that stops the second report is the state a recovery must clear; a watch
    /// that only latched would witness one hang per process lifetime, which for
    /// an intermittent fault is the wrong one.
    #[test]
    fn a_second_stall_after_a_recovery_earns_its_own_report() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait)),
            Verdict::Quiet
        );
        assert!(matches!(
            watch.poll(7_000, pulse(900, 1, Station::Present)),
            Verdict::Hung { .. }
        ));
        assert!(matches!(
            watch.poll(9_000, pulse(8_000, 2, Station::Wait)),
            Verdict::Healed { .. }
        ));
        assert!(matches!(
            watch.poll(15_000, pulse(8_000, 2, Station::WebPage)),
            Verdict::Hung {
                station: Station::WebPage,
                ..
            }
        ));
    }

    /// PIN — **liveness is the turn counter, not the clock.** A loop that keeps
    /// stamping the same turn is a loop that has not come round, however fresh
    /// its timestamp looks; and a turn counter that moves while the timestamp
    /// stands still is still a live pump.
    #[test]
    fn the_pump_is_alive_when_its_turn_moves_and_not_when_its_clock_does() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(
            watch.poll(1_000, pulse(1_000, 1, Station::Wait)),
            Verdict::Quiet
        );
        // Same turn, ancient stamp: a hang.
        assert!(matches!(
            watch.poll(9_000, pulse(1_000, 1, Station::Wait)),
            Verdict::Hung { .. }
        ));
        // A different watch: the turn moves but the stamp does not advance —
        // two turns inside one millisecond, which is what a busy loop looks
        // like. That is a live pump and owes no report.
        let mut busy = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(
            busy.poll(1_000, pulse(1_000, 1, Station::Wait)),
            Verdict::Quiet
        );
        assert_eq!(
            busy.poll(9_000, pulse(1_000, 2, Station::Wait)),
            Verdict::Quiet
        );
    }

    /// PIN (real run, 2026-08-25) — **a start is judged against its own, longer
    /// grace, and a start that never finishes is still a hang.**
    ///
    /// Both halves come from the same evidence. The first run of this watchdog
    /// on a real window filed a report at station `starting` because a cold
    /// debug launch took eight seconds to reach `about_to_wait` — a false alarm
    /// on a program that was working, and one that would land on every cold
    /// boot. The second half is why the answer is not to ignore startup: a
    /// window that never appears is the loudest hang there is.
    #[test]
    fn a_start_is_judged_against_its_own_grace_and_still_reported_if_it_never_ends() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(
            watch.poll(2_000, pulse(0, 0, Station::Starting)),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(8_000, pulse(0, 0, Station::Starting)),
            Verdict::Quiet,
            "eight seconds to build an event loop, a GPU device and a shell is a \
             slow start, not a hang — this is the false alarm the first real run filed"
        );
        assert_eq!(
            watch.poll(31_000, pulse(0, 0, Station::Starting)),
            Verdict::Hung {
                silent_ms: 31_000,
                threshold_ms: 30_000,
                station: Station::Starting,
                turn: 0,
            },
            "but a start that has gone half a minute is reported, against the \
             threshold it actually crossed"
        );
    }

    /// PIN — **the grace is spent once.** A loop that has taken one turn has
    /// proved it can, so every stall after that is judged at five seconds — a
    /// watch that kept the startup grace would sleep through the first real
    /// hang of every run.
    #[test]
    fn a_loop_that_has_turned_once_is_judged_at_the_ordinary_threshold() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait)),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(7_000, pulse(900, 1, Station::Drain)),
            Verdict::Hung {
                silent_ms: 6_100,
                threshold_ms: 5_000,
                station: Station::Drain,
                turn: 1,
            }
        );
    }

    /// PIN — **the heartbeat's three words survive a round trip**, including
    /// the station byte, which is the one part of the evidence that exists when
    /// nothing else does.
    #[test]
    fn a_heartbeat_reports_the_last_station_it_was_given() {
        let heart = Heartbeat::new();
        assert_eq!(heart.sample().station, Station::Starting);
        assert_eq!(heart.sample().turn, 0);
        heart.beat();
        assert_eq!(heart.sample().turn, 1);
        assert_eq!(heart.sample().station, Station::Wait);
        heart.at(Station::PtyResize);
        assert_eq!(heart.sample().station, Station::PtyResize);
        heart.beat();
        assert_eq!(
            heart.sample().station,
            Station::Wait,
            "a new turn returns the loop to its own station"
        );
        assert_eq!(heart.sample().turn, 2);
        for station in [
            Station::Starting,
            Station::Wait,
            Station::Event,
            Station::Drain,
            Station::PtyResize,
            Station::Present,
            Station::Wheel,
            Station::WebPage,
            Station::Autosave,
            Station::SelfTest,
        ] {
            heart.at(station);
            assert_eq!(
                heart.sample().station,
                station,
                "every station survives the byte it is stored as"
            );
        }
    }

    /// PIN — **the report names the station, the silence and the stack**, and
    /// its counter footer distinguishes a clean run from a silent one.
    #[test]
    fn a_report_names_the_station_the_silence_and_the_stack() {
        let stack = bt_platform::hang::StackSample {
            rip: 0x1_0000_0100,
            rsp: 0x8000,
            rip_site: Some(bt_platform::hang::ModuleSite {
                address: 0x1_0000_0100,
                module: "folio.exe".to_owned(),
                offset: 0x100,
                depth: 0,
            }),
            frames: vec![bt_platform::hang::ModuleSite {
                address: 0x7fff_0000_0040,
                module: "ntdll.dll".to_owned(),
                offset: 0x40,
                depth: 24,
            }],
            scanned_bytes: 4096,
            modules: 120,
            note: None,
        };
        let report = render_report(&ReportFacts {
            written_at: "2026-08-25T04:05:06.007Z",
            process_id: 4242,
            ui_thread_id: 91,
            uptime_ms: 41_250,
            silent_ms: 10_300,
            threshold_ms: 5_000,
            station: Station::SelfTest,
            turn: 4172,
            stack: &stack,
            surfaces: bt_render::SurfaceFailureTally {
                outdated: 3,
                ..bt_render::SurfaceFailureTally::default()
            },
        });
        assert!(report.contains("pid 4242, ui thread 91"), "{report}");
        assert!(
            report.contains("pump silent for: 10.300s (threshold 5.000s)"),
            "{report}"
        );
        assert!(
            report.contains("last station   : BT_HANG_SELFTEST"),
            "{report}"
        );
        assert!(report.contains("loop turns     : 4172"), "{report}");
        assert!(report.contains("rip    : folio.exe+0x100"), "{report}");
        assert!(report.contains("[+0x00018] ntdll.dll+0x40"), "{report}");
        assert!(
            report.contains("outdated 3"),
            "the run counters are part of the report, {report}"
        );
        assert!(
            !report.contains("clean"),
            "a run with three absorbed failures does not claim to be clean, {report}"
        );
        let clean = render_report(&ReportFacts {
            written_at: "2026-08-25T04:05:06.007Z",
            process_id: 1,
            ui_thread_id: 2,
            uptime_ms: 0,
            silent_ms: 0,
            threshold_ms: 0,
            station: Station::Drain,
            turn: 0,
            stack: &stack,
            surfaces: bt_render::SurfaceFailureTally::default(),
        });
        assert!(
            clean.contains("surface acquires: clean"),
            "and a run with none says so in one word, {clean}"
        );
        assert_eq!(
            render_healed(8_500, Station::Drain),
            "\nhealed         : the pump came back after 8.500s at drain_pty\n"
        );
    }

    /// PIN — **a report whose stack could not be taken is still a report.** The
    /// station label is the floor of the evidence, and a capture that was
    /// refused says which call refused it rather than presenting an empty list
    /// as if the stack had been empty.
    #[test]
    fn a_refused_capture_still_yields_a_report_with_a_station_in_it() {
        let stack = bt_platform::hang::StackSample {
            note: Some("GetThreadContext was refused"),
            ..bt_platform::hang::StackSample::default()
        };
        let report = render_report(&ReportFacts {
            written_at: "2026-08-25T04:05:06.007Z",
            process_id: 1,
            ui_thread_id: 2,
            uptime_ms: 1_000,
            silent_ms: 6_000,
            threshold_ms: 5_000,
            station: Station::Drain,
            turn: 9,
            stack: &stack,
            surfaces: bt_render::SurfaceFailureTally::default(),
        });
        assert!(
            report.contains("note   : GetThreadContext was refused"),
            "{report}"
        );
        assert!(report.contains("last station   : drain_pty"), "{report}");
        assert!(
            report.contains("(no module-resolvable addresses on the stack)"),
            "{report}"
        );
    }

    /// PIN — **a report's name sorts chronologically as text**, which is the
    /// whole basis on which the pruner decides what is oldest.
    #[test]
    fn a_report_name_sorts_by_time_as_plain_text() {
        assert_eq!(
            report_filename("2026-08-25T04:05:06.007Z"),
            "hang-20260825040506007.txt"
        );
        let mut names = [
            report_filename("2026-12-31T23:59:59.999Z"),
            report_filename("2026-08-25T04:05:06.007Z"),
            report_filename("2027-01-01T00:00:00.000Z"),
        ];
        names.sort();
        assert_eq!(
            names,
            [
                "hang-20260825040506007.txt",
                "hang-20261231235959999.txt",
                "hang-20270101000000000.txt"
            ]
        );
    }

    /// PIN — **the timestamp is a real calendar**, sharing the git panel's
    /// Gregorian arithmetic rather than an approximation of it.
    #[test]
    fn the_timestamp_is_the_gregorian_calendar_and_not_an_approximation() {
        assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            utc_timestamp(UNIX_EPOCH + Duration::from_millis(1_774_400_706_007)),
            "2026-03-25T01:05:06.007Z"
        );
        assert_eq!(
            utc_timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00.000Z",
            "2000 is a leap year and 1900 was not — the case a naive rule gets wrong"
        );
        // Not a panic on a clock that has gone backwards past the epoch.
        assert_eq!(
            utc_timestamp(SystemTime::UNIX_EPOCH - Duration::from_secs(10)),
            "1970-01-01T00:00:00.000Z"
        );
    }

    /// PIN — **the cap holds and it only ever deletes our own files.** A pruner
    /// that swept the directory would be a diagnostic that eats whatever a
    /// person put beside its output.
    #[test]
    fn pruning_keeps_the_newest_and_touches_nothing_that_is_not_ours() {
        let directory = std::env::temp_dir().join(format!(
            "folio-hang-prune-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a private directory for this test");
        for index in 0..5 {
            std::fs::write(
                directory.join(format!("hang-2026082504050600{index}.txt")),
                "x",
            )
            .expect("a report");
        }
        std::fs::write(directory.join("notes.txt"), "mine").expect("a note");
        std::fs::write(directory.join("hang-something.log"), "not ours").expect("a log");

        assert_eq!(prune_reports(&directory, 2).expect("prune"), 3);
        let mut left: Vec<String> = std::fs::read_dir(&directory)
            .expect("read back")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "hang-20260825040506003.txt",
                "hang-20260825040506004.txt",
                "hang-something.log",
                "notes.txt",
            ],
            "the two newest reports, and both of the files that are not reports"
        );
        assert_eq!(
            prune_reports(&directory, 2).expect("prune"),
            0,
            "a directory already at the cap loses nothing"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
