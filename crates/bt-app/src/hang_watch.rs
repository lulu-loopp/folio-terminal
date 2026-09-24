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
//! The window thread stamps a heartbeat once per turn of `about_to_wait`,
//! leaves a **station** label — one byte, one store — at the door of each of the
//! long calls it makes, and before handing control back to the platform records
//! **how long it means to be away**. A watchdog thread wakes every two seconds
//! and asks three questions: has the turn counter moved; if not, was the thread
//! due back; and if it was due back, does it still answer a message. Only when
//! all three say no does it suspend the window thread for exactly two kernel
//! calls, read its registers and its raw stack (see [`bt_platform::hang`]),
//! resume it, and write what it found — **the station, the silence, the parking,
//! the answer, the stack, and the run's counters** — to a file under
//! `%APPDATA%\Folio\hang-reports\`.
//!
//! # Liveness is "due back and answering", not "busy"
//!
//! The first version of this file asked one question — *did the loop come
//! round?* — and it was the wrong one. An event-driven GUI with nothing to do
//! **legitimately stops turning its loop**: `about_to_wait` sets
//! `ControlFlow::Wait`, the thread parks in `NtUserMsgWaitForMultipleObjectsEx`,
//! and it stays there for as long as nobody types. A watchdog that read that as
//! a stopped pump filed a report every eight seconds at whatever function
//! happened to be the last one tagged on the quiet turn before it — 200 of the
//! first 205 reports this facility ever wrote were exactly that, and the station
//! they blamed (`flush_pending_pty_resize`) had returned successfully long
//! before the silence began.
//!
//! So the ground truth is two facts the window thread and the platform can both
//! be held to:
//!
//! 1. **Was it due back?** Handing control to the platform, the thread records
//!    its own parking: a deadline for `ControlFlow::WaitUntil`, "indefinitely"
//!    for `ControlFlow::Wait`. Silence past a deadline is a wake-up the platform
//!    promised and did not deliver.
//!
//!    Silence inside an **indefinite** park is not a hang by arithmetic — nothing
//!    was owed — and until R2-28 that was the end of it: the arm answered
//!    quiet and the question below was never put. What that missed is the state
//!    an indefinite park most often ends in badly: a wake arrives, and the thread
//!    wedges on the way out of the park before it writes a new pulse. The park
//!    reason it left behind is then read for ever, and no report is written.
//!    So the park reason now decides what the *arithmetic* means and no longer
//!    decides whether to ask: past the threshold the thread is asked, whatever it
//!    said on its way out, and an idle window answers and is excused. See
//!    `HangWatch::asked_at_ms` for the cadence that keeps that from being a
//!    message to an idle program every two seconds.
//! 2. **Does it answer?** Suspicion is not a verdict. Before a report is
//!    written the watchdog *asks* — one `WM_NULL` with a bounded wait, see
//!    [`bt_platform::hang::ask_thread_to_answer`] — and a thread that replies is
//!    alive whatever its loop is doing. That is also what makes a USER32 modal
//!    loop (dragging a window edge, a tracked menu) correct rather than merely
//!    tolerated: the application really is not turning winit's loop, and it
//!    really is not hung.
//!
//! A real wedge answers both questions the wrong way and is still caught: it
//! never reached the hand-back, so it is not parked and its station is the call
//! it is stuck in; and it is not pumping, so it does not answer. The measured
//! one — 131 seconds, `IsHungAppWindow` true, the UI thread burning a core —
//! is exactly that shape.
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
//! - **Per station transition**, one monotonic-clock read, relaxed atomic
//!   accounting and a fixed-capacity call-tree lookup. An enter/leave pair has
//!   two clock reads, no allocation and no system call. The previous ledger
//!   already read the clock and charged exclusive time; it was not two stores.
//!   Call-tree keys include the parent and optional pane ID, so repeated calls
//!   do not borrow another event's milliseconds. Overflow keeps the full coarse
//!   ledger and explicitly marks the line. Formatting stays on the watchdog.
//! - **CPU time**, one read when a hold opens while the watch is armed, and a
//!   second only when a slow hold is admitted to the reporting queue. Neither
//!   sample is a process CPU counter; a failed sample prints no invented zero.
//! - **Per turn of the loop**, [`beat`] is one `Instant::now()` (which
//!   `about_to_wait` already calls for its own clocks) plus four stores, and
//!   [`park`] at the other end of the turn is two more. The footprint baseline
//!   is cached for [`FOOTPRINT_SAMPLE_INTERVAL`], bounding its one kernel query
//!   to four a second even when a platform spuriously spins its run loop. A
//!   parked thread makes none of them: no hold is open, so nothing is sampled.
//! - **Per two seconds, forever**, the watchdog does one `Instant::now()`, four
//!   atomic loads and a comparison, then sleeps again. **Zero allocation**: the
//!   idle path never touches the heap, never opens a file, and never creates the
//!   reports directory — a run that does not hang leaves nothing on the disk at
//!   all. The one message it can send costs nothing until the arithmetic has
//!   run out of innocent explanations, which on an idle window is never.
//! - **Per report**, and only then: a module enumeration, a 128 KiB read and a
//!   file write, all on the watchdog thread. Nothing is ever written from the
//!   window thread.
//!
//! # The stall that is over before this watchdog looks
//!
//! Everything above is built for a wedge — five seconds of silence from a thread
//! that will not answer. **The fault a person actually reports is smaller than
//! that and it heals**: the window goes dead for two or three seconds, the tab
//! they clicked does not switch, and by the time they have looked at the log it
//! is working again. Two separate rules above make that invisible. It may not
//! reach [`HANG_THRESHOLD`] at all; and even when it does, a thread blocked
//! inside a cross-apartment COM call is *pumping messages while it waits*, so it
//! answers the watchdog's `WM_NULL` and is filed as [`Verdict::Excused`] —
//! which says nothing out loud, on purpose, because that is also what a window
//! being dragged looks like from the outside.
//!
//! So there is a second instrument, and it measures from the inside. The window
//! thread already tells this module when it takes control ([`Heartbeat::woke`])
//! and when it hands it back ([`Heartbeat::park`]); between those two it passes
//! through the stations. Timing the stations turns "the last call it entered"
//! — a floor, and the honest floor for a thread that cannot be asked — into an
//! **account of where a hold's milliseconds went**, and a hold that ran past
//! [`SLOW_HOLD_THRESHOLD`] is written to the diagnostics log as one line naming
//! the stations in order of what they cost.
//!
//! Three things follow from measuring it this way rather than from outside:
//!
//! * **It cannot be excused.** The measurement is taken by the thread that was
//!   blocked, so a COM call that pumped, a modal loop and a wedge all read the
//!   same: control was held this long, and here is where.
//! * **It reports after the fact.** The line is produced at [`Heartbeat::park`],
//!   which is the first moment the length of the hold is known — so a stall that
//!   heals leaves a line and one that never heals leaves the watchdog's report
//!   instead. The two instruments cover each other exactly.
//! * **It is not a verdict.** A window being dragged by its edge really does
//!   hold control for as long as the drag, and a line saying so is a true
//!   sentence about a program that is working. This is a hunting instrument, and
//!   a hunting instrument that hid true measurements to keep its log tidy would
//!   be the reason the next report has nothing in it either.
//!
//! The bill: [`at`] grows from two relaxed stores to one clock read, one
//! `fetch_add` and three stores — still no allocation, no fence and no branch
//! that can block, and still far less than the call it stands in front of. The
//! line itself is formatted and written on the watchdog thread, never on the
//! window thread; the window thread's whole part is a `try_lock` it never waits
//! on and a `push`.
//!
//! # A run that asked to be measured is judged sooner
//!
//! The two instruments above cover each other exactly only while a stall is
//! either shorter than [`SLOW_HOLD_THRESHOLD`] or longer than
//! [`HANG_THRESHOLD`]. The owner's traced run of 2026-09-15 landed between
//! them — `held control for 3971 ms on turn 15341 — window_event 3960 ms` —
//! and that line is the whole of what this facility could say about it: four
//! seconds went into an event. Four seconds is a dead window by any reader's
//! account, and the question the line leaves is *where inside the event*, which
//! nothing but a stack answers.
//!
//! So a run started with `BT_PERF_TRACE` set is judged against
//! [`TRACED_HANG_THRESHOLD`] — two seconds — and every other run against
//! [`HANG_THRESHOLD`]. It is the only thing that variable changes in here, and
//! it is opt-in for the reason the rest of this facility is resident: a report
//! suspends the window thread for two kernel calls and leaves somebody a file,
//! and a machine that stalls for two seconds under a compile asked for neither.
//! A machine whose owner set the variable did. The variable is read once, on
//! the window thread, beside every other reader of it, and handed to [`start`];
//! the watchdog thread asks the environment nothing, and a report prints the
//! threshold it actually crossed.
//!
//! # Whose seconds they were
//!
//! A line reading `flush_wheel 1928 ms` names where the time went and cannot
//! say **whose time it was**. Two seconds inside one call is either two seconds
//! of this program's own work, which is repaired here, or two seconds of this
//! program standing still while the operating system reads its working set back
//! in, which is not a fault in this program at all — and the machines where
//! this instrument earns its keep are exactly the ones carrying more committed
//! memory than they have RAM. The two demand opposite repairs and, until the
//! counters below, the log could not tell them apart.
//!
//! So a hold now also carries [`Paging`]: the process's page faults and
//! resident size, sampled around the hold and appended to the line after a
//! middle dot. The opening baseline is at most
//! [`FOOTPRINT_SAMPLE_INTERVAL`] old; the closing sample is taken only for a
//! hold that is being reported. *Slow* is not known until a hold ends, while a
//! baseline read after the paging is over measures nothing. See
//! [`Heartbeat::open_footprint`] for the bounded compromise.
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
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bt_platform::mem::Footprint;

pub use bt_platform::hang::Answer;

/// How often the watchdog looks.
///
/// Two seconds: fast enough that a five-second threshold is crossed within
/// seven, slow enough that the whole idle cost of this facility is 30 atomic
/// loads a minute.
const WATCH_INTERVAL: Duration = Duration::from_secs(2);

/// How long the window is given to answer the watchdog's question before its
/// silence is taken for the fault.
///
/// One second, which is half a poll interval: long enough that a thread merely
/// busy with a frame gets its reply in — a `WM_NULL` is answered between two
/// messages, not after the current one finishes — and short enough that the
/// asking never delays the poll that owes a report.
const ANSWER_WITHIN: Duration = Duration::from_secs(1);

/// How long the pump may be silent before it is a hang.
///
/// Five seconds because that is well past anything the loop legitimately does —
/// the longest measured single-turn cost in the perf-resilience work (§1.4) was
/// a 1.25 s frame under 24-way `cargo` — and it is also the neighbourhood where
/// Windows itself starts drawing the ghost window and saying `Not Responding`,
/// which is the symptom the user reports.
///
/// **The threshold an ordinary run is judged against.** A run that was started
/// in order to be measured is judged against [`TRACED_HANG_THRESHOLD`] instead,
/// and which of the two applies is settled once, in [`start`].
const HANG_THRESHOLD: Duration = Duration::from_secs(5);

/// How long the pump may be silent before it is a hang, **on a run that asked
/// to be measured** (`BT_PERF_TRACE`).
///
/// Two seconds. The two instruments this module carries cover each other
/// exactly only while a stall is either shorter than [`SLOW_HOLD_THRESHOLD`] or
/// longer than [`HANG_THRESHOLD`], and the owner's traced run of 2026-09-15
/// landed between them: `the window thread held control for 3971 ms on turn
/// 15341 — window_event 3960 ms`. The ledger named the lane, and the watchdog —
/// never past its own threshold — took no stack, so the one question that line
/// leaves had no answer in the run that produced it.
///
/// Not a threshold an ordinary run could carry. A report suspends the window
/// thread for two kernel calls and writes a file, and a person whose machine
/// stalls for two seconds under a compile asked for neither; a person who set
/// `BT_PERF_TRACE` asked for exactly that, and this is the only thing the
/// variable changes in this module.
///
/// Two and not one, because the watchdog wakes every [`WATCH_INTERVAL`]: a
/// threshold shorter than the poll would be crossed and gone before anything
/// looked at it, and two seconds is crossed within four.
const TRACED_HANG_THRESHOLD: Duration = Duration::from_secs(2);

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

/// **How long the window thread may hold control before the run writes down
/// where the time went.**
///
/// Half a second, which is two orders of magnitude below [`HANG_THRESHOLD`] and
/// deliberately so: this instrument is not looking for a wedge — that one has
/// its own — but for the stall a person notices and the watchdog above either
/// misses or excuses. Half a second is also comfortably past everything the
/// loop legitimately does per turn (the longest measured ordinary turn in the
/// perf-resilience work was 1.25 s, and that was a debug build under a 24-way
/// `cargo`, which is exactly the kind of hold worth a line).
const SLOW_HOLD_THRESHOLD: Duration = Duration::from_millis(500);
/// A footprint baseline may be this old when a hold opens. Half the slow-hold
/// threshold keeps the attribution useful while bounding the platform query to
/// four calls a second during a busy or spuriously woken loop.
const FOOTPRINT_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// How many slow holds may wait for the watchdog to write them.
///
/// The queue is drained every [`WATCH_INTERVAL`], so it only fills when the
/// window thread is producing slow holds faster than one every 60 ms for two
/// solid seconds — a program that is on fire, where the thirty-third line adds
/// nothing the first thirty-two did not. Overflow is counted rather than
/// silently dropped.
const SLOW_HOLDS_KEPT: usize = 32;

/// [`SLOW_HOLD_THRESHOLD`], in the milliseconds a [`Heartbeat`] counts.
///
/// One conversion in one place, so the constant above is the only number and
/// the ledger cannot come to be compared against a different one.
#[must_use]
fn slow_hold_threshold_ms() -> u64 {
    u64::try_from(SLOW_HOLD_THRESHOLD.as_millis()).unwrap_or(u64::MAX)
}

/// How many stations there are, and therefore how wide one hold's ledger is.
///
/// Held against [`Station`] by `every_station_has_a_slot_in_the_ledger`: a
/// further variant added without widening this would have its milliseconds
/// charged to nobody, and the line would silently stop adding up.
const STATION_COUNT: usize = 207;

#[path = "hang_watch_detail.rs"]
mod detail;

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
    /// Inside `window_event`, and **outside the handler the event went to**:
    /// the id lookup, the three gates a retiring or leaving window is refused
    /// at, the wheel burst spent before anything that is not a notch, and the
    /// four application doors the dispatch closes with.
    ///
    /// **It used to be the whole of it** (T-WINDOW-EVENT-STATIONS). Every kind
    /// winit delivers wore this one word, so the owner's traced run of
    /// 2026-09-15 — `held control for 3971 ms on turn 15341 — window_event
    /// 3960 ms` — said that four seconds had gone into *an event* and could not
    /// say which: a keystroke, a wheel notch, a redraw and a resize are four
    /// lanes, repaired four different ways. The thirteen stations at the foot
    /// of this enum are those lanes.
    Event = 2,
    /// `Runtime::drain_pty` — every shell's output, a slice at a time until the
    /// turn's quantum or its millisecond budget runs out (T-DRAIN-BURST). It is
    /// this station's own measurements — `drain_pty 4386 ms` with the page-fault
    /// column near zero — that put the clock there.
    Drain = 3,
    /// `Runtime::flush_pending_pty_resize` — the synchronous `ResizePseudoConsole`
    /// round trip into conhost, one per pane per quiet window.
    PtyResize = 4,
    /// `Runtime::publish_frame_inner` — compose, acquire, submit, present.
    Present = 5,
    /// `Runtime::flush_wheel` — a coalesced burst of notches being spent.
    Wheel = 6,
    /// `Runtime::flush_dropped_files` — the paths one drop put on this window,
    /// spelled for a shell and written into it (GitHub issue #1 ②).
    ///
    /// [`Self::Wheel`]'s twin, one gesture over and collected the same way:
    /// winit delivers a dropped file per event with no batch marker, so the
    /// paths accumulate until the turn boundary and are spent here, once. Its
    /// own station rather than [`Self::EventFileDrop`]'s for the reason the
    /// wheel's pair states — one is an addition to a list and the other is a
    /// write onto a child's input, and the two are repaired differently.
    FileDrop = 48,
    /// `Runtime::advance_web_page` — a call into WebView2 and therefore into
    /// another process.
    WebPage = 7,
    /// `Runtime::sync_web_page` — the placement calls a **frame** makes into
    /// every page whose rectangle moved: `SetRasterizationScale`, `SetBounds`,
    /// `SetIsVisible`. Its own station rather than [`Self::WebPage`]'s because
    /// the two are reached on different clocks and would be repaired
    /// differently: this one is paid per animated frame per moving page, and
    /// [`Self::WebRetire`] is paid once when a page goes.
    WebPlace = 12,
    /// `ICoreWebView2Controller::Close` — the one synchronous call into the
    /// browser on the path a closing tab takes. Everything else in
    /// [`Self::WebPage`] is arithmetic and a deadline comparison, so a hold
    /// that lands here has named a very short piece of code.
    WebRetire = 13,
    /// `Runtime::apply_web_outcomes` — what a turn does with what the engine
    /// said since the last one. **The heavy arm is a controller arriving**: a
    /// fresh one is configured with a dozen synchronous property calls and then
    /// navigated, all on this thread.
    ///
    /// Its own station because of what the first machine run of this ledger
    /// caught: 659 ms charged to `advance_web_page` on the turn a PDF's engine
    /// came up, with no way to tell the lifecycle clock from the configuring
    /// burst. Two stations is the difference between a measurement and a repair.
    WebOutcomes = 14,
    /// `SessionStore::flush_if_due` — the autosave's own door.
    Autosave = 8,
    /// The deliberate hang of [`run_selftest_if_due`]. Debug builds only.
    SelfTest = 9,
    /// **Control has been handed back to the platform.** Stamped at the end of
    /// every turn, so a thread that is merely idle reports *that* rather than
    /// whichever long call happened to be the last one tagged before it went
    /// quiet — which is the misattribution that put `flush_pending_pty_resize`
    /// on two hundred reports about a window nobody was using.
    Parked = 10,
    /// The platform delivered a wake and the loop has not reached a named call
    /// yet. The honest label for a thread that has left [`Self::Parked`] and
    /// has not arrived anywhere else.
    ///
    /// **Since 2026-09-11 this is a much narrower claim than it was.** A wake
    /// carrying a worker's answer used to spend the whole of `user_event` here,
    /// so `woken 85613 ms` was the most this ledger could say about a stall that
    /// had a perfectly good name — see the eight stations below, and
    /// [`crate::AppEvent::station`], which is where each of them is chosen. What
    /// is left against this label is a wake that named no lane: the arms that do
    /// nothing because `about_to_wait` is about to do it, and any untagged code
    /// on the way in.
    ///
    /// **And narrower again since T-STATION-SPLIT.** A hold *opens* at this
    /// station, so everything the loop did before it reached its first named
    /// call was charged here — which was the whole of `about_to_wait`'s
    /// application prologue. That has its own name now
    /// ([`Self::AppTurn`]), and what is left is the wake itself.
    Woken = 11,
    /// `Runtime::apply_preview_results` — a preview read landing: the head of a
    /// file, the whole of one a reader asked to edit, a picture's pixels, an
    /// animation's next frames. **The heavy arm is the document that follows**:
    /// a body that arrives is parsed, measured and laid out before this returns.
    Preview = 15,
    /// `Runtime::apply_math_results` — one batch of rendered formulas, and the
    /// re-measure of every block that was waiting for a picture's size.
    Math = 16,
    /// `Runtime::apply_files_results` — a directory listing landing in the files
    /// column, with the sort and the row build that follow it.
    Files = 17,
    /// `Runtime::apply_git_results` — one repository's status or graph.
    Git = 18,
    /// `Runtime::raise_attention` — what the listener heard, read into the tabs
    /// that were waiting to be told.
    Attention = 19,
    /// `Runtime::adopt_background_picture` — the window ground a decoded picture
    /// is put in force around.
    Picture = 20,
    /// `Runtime::apply_file_index_results` — the palette's index of a folder.
    FileIndex = 21,
    /// The probe and settings family — `PsReadLineProbed`,
    /// `PowerShellProfileProbed`, `CopilotProbed`, `UpdateChecked`,
    /// `ExplorerPackageChanged`, `FontsScanned`, `SchemesChanged`,
    /// `StorageChanged`, `SystemPreferencesChanged`, `NotificationClicked`.
    ///
    /// **One station for nine arms**, because what they have in common is what a
    /// reader of this line needs: every one of them ends in `refresh_chrome` over
    /// every window, and none of them is on a clock a reader can feel. A stall
    /// here says "something a probe answered", which is as far as this ledger can
    /// usefully divide a family that costs nothing — and further splitting is a
    /// line to add on the day one of them is the answer.
    ///
    /// **And since T-STATION-SPLIT the label is literally true**:
    /// `Runtime::refresh_chrome` enters this station itself and hands the
    /// caller's back on the way out, so the rebuild is named at *every* one of
    /// its two hundred-odd doors rather than only on the probes' road. It is
    /// the heaviest call a keystroke or a hover can make outside a frame — the
    /// strip, the rail, every pane head, every preview card's measured verb and
    /// the focus column's thumbnails, all rebuilt — and until then it borrowed
    /// whichever name happened to be standing when it was reached.
    Chrome = 22,
    /// `Runtime::drive_web_page` — the turn a hosted page's callbacks are read
    /// on. Its own station rather than [`Self::WebPage`]'s, which is
    /// `advance_web_page` and is reached from `about_to_wait` on a different
    /// clock; what this one does *not* cover is the arms inside it that name
    /// themselves ([`Self::WebOutcomes`], [`Self::WebRetire`]), so what is left
    /// against it is the drive itself and the chrome read that follows.
    WebSpoke = 23,
    /// `Runtime::settings_layout` — the Settings dialog's contents and geometry,
    /// rebuilt from scratch on every road that draws, hovers or hit-tests it,
    /// and once more on the press that opens it.
    ///
    /// **Born naming a stall that had already been reported** (GitHub issue #3).
    /// An outside user's window froze for seconds when the gear was clicked, and
    /// this ledger could only say `window_event` — the press — or
    /// `publish_frame_inner` — the frame it happened to be inside. Both were
    /// true and neither named the dialog. The cause was one call in this lane
    /// walking the machine's font collection; the walk has gone to a worker, and
    /// what is left here is what this page honestly costs: the rows, every
    /// picker's width measured through the renderer, and the profile and scheme
    /// lists it reads afresh each time.
    Settings = 24,
    /// Synchronous clipboard acquisition may wait on another process's delayed renderer.
    ClipboardRead = 25,
    /// `Runtime::settle_deferred_dpi` and `Runtime::settle_dpi_rectangle` — the
    /// two halves of a display change, taken at the top of a turn.
    ///
    /// A no-op on almost every turn and expensive on the ones it is not: the
    /// window's font is re-measured at the new scale, every pane is re-solved
    /// and the shells are told their new grids. Its own station because it sits
    /// between [`Self::Wheel`] and [`Self::Drain`], where a hold used to be
    /// charged to a wheel nobody had touched.
    DpiSettle = 26,
    /// `Runtime::apply_math_context_menu_result`,
    /// `Runtime::apply_folder_pick_result` and
    /// `Runtime::apply_image_pick_result` — what a turn does with the answer a
    /// modal the platform owns left behind.
    ///
    /// **One station for three arms**, on [`Self::Chrome`]'s reasoning: all
    /// three are a no-op unless a dialog was up, all three end in a path being
    /// opened or a formula being written, and a reader who sees time here has
    /// the one fact they need — a picker had just closed.
    Pickers = 27,
    /// `Runtime::settle_pane_notices` and `Runtime::settle_preview_rails` — the
    /// two rows a turn can add to or take from a pane.
    ///
    /// Grouped because they are the same kind of change and cost the same kind
    /// of work: a row appearing or going is a pane's height changing, which
    /// re-solves the seat and re-measures the grid behind it.
    PaneRows = 28,
    /// Every "has anything changed out there" poll a turn makes:
    /// `advance_scheme_watch`, `advance_storage_watch`, `advance_preview_watch`,
    /// `advance_files_watch` and `advance_git_watch`.
    ///
    /// **One station for five polls**, which is [`Self::Chrome`]'s judgement
    /// again and for a sharper reason: what they have in common is the thing
    /// that can make one of them slow, which is a disk that has stopped
    /// answering. A hold here says "a watch was asking the file system", and
    /// which watch it was is the stack's question rather than this label's.
    ///
    /// They are not contiguous in `turn` — two of them stand on the application
    /// clock and three do not — so this station is entered more than once on a
    /// turn that runs them all, and the ledger adds the pieces up.
    Watches = 29,
    /// `Runtime::finish_synchronized_update_if_due` — the end of a DEC 2026
    /// block, where a screenful of output a program asked to have held back is
    /// handed to the grid in one go.
    ///
    /// Its own station because it is the one call in the clock run that can be
    /// handed an unbounded amount of text: everything else on that lane is a
    /// deadline comparison, and this one is a feed.
    SyncUpdate = 30,
    /// **Every clock a window keeps**, run in order once a turn: the first-run
    /// and PSReadLine invitations, the cursor and rename blinks, the tab press,
    /// the strip animation, the composition owner and the IME caret, the resize
    /// and preview-scale settlements, the live-math and hover clocks, the four
    /// menus, the drag spring and its autoscroll, the maths toggle and its
    /// tools, the layout peek, the tooltip, the key and card hints, the toasts,
    /// the command flash and rails, the terminal thumbs, the file peek, the
    /// float, the foot reveal, the page feet and the preview notices.
    ///
    /// **Born to take forty-eight calls off the autosave's name**
    /// (T-STATION-SPLIT). `SessionStore::flush_if_due` names itself
    /// ([`Self::Autosave`]) and was the last station entered before this whole
    /// run, so a hold anywhere in it was reported as the autosave — a lane that
    /// writes one small file and had nothing to do with any of it.
    ///
    /// One station for the run rather than one per clock, because what they
    /// have in common is exactly what a reader needs: each is an `if due` over a
    /// deadline this window set, each ends in the chrome being rebuilt, and none
    /// of them waits on anything outside this process. A hold here is this
    /// window drawing itself, and the stack says which clock.
    Clocks = 31,
    /// The arithmetic at the foot of a turn that decides when the loop should be
    /// woken again — every clock's deadline read and the earliest of them taken.
    ///
    /// Its own station so that [`Self::PtyResize`] names the synchronous
    /// `ResizePseudoConsole` round trip it was built to name, and nothing else.
    /// Nothing here can block; time against it is a turn that is doing
    /// arithmetic over a great many windows, and that is worth being able to
    /// see rather than to assume.
    Deadlines = 32,
    /// `FolioApp::about_to_wait_inner`'s own prologue — everything one turn owes
    /// the *application* before any window takes its turn: the window directory,
    /// the ring, an application change, a restore answer, a drag handed over, a
    /// window asked for or launched, the delegate's events, the summoned
    /// terminal, the quit, the menu bar, the drag broker, and the reaping of
    /// windows that have left.
    ///
    /// **It was the last thing left under [`Self::Woken`]** (T-STATION-SPLIT).
    /// A hold opens at the wake and the station is stamped `Woken` there, so
    /// until this variant every one of those calls was reported as a wake that
    /// named no lane — including the two that are by far the most expensive
    /// things this loop can do on a turn: **opening** a window, which builds a
    /// surface and a swapchain, and **reaping** one, which shuts its shells and
    /// waits for its pages.
    AppTurn = 33,
    /// `search::scan_history` and `search::scan_volatile` — the capsule's own
    /// regular expression run over this pane's whole transcript.
    ///
    /// **The one piece of work on the typing path that is O(the document)**
    /// (T-STATION-SPLIT). A terminal's find is live by design: every keystroke
    /// in the box bumps the search revision, which is what makes the cached
    /// history hits unusable, so every keystroke re-runs the pattern over every
    /// frozen line — a hundred thousand of them at the default scrollback. It is
    /// the right answer to the right question and it is not a fault; what it was
    /// missing was a name, and without one it was reported as whatever call had
    /// last been tagged, which on the owner's recording of 2026-09-15 was
    /// `flush_wheel` while the hand was typing and the wheel was untouched.
    ///
    /// Entered and left around the two scans themselves rather than around
    /// `refresh_search`, which leaves through eight doors: a station that is put
    /// back on only one of them would be a worse lie than the one this replaces.
    SearchScan = 34,
    /// `WindowEvent::CloseRequested` — the dirty gate, which asks the reader
    /// about preview buffers that would not survive the shut, and the summoned
    /// terminal's `×`, which sets a bit and returns.
    ///
    /// **The head of the window-event family, whose one rule is stated here.**
    /// Each of the thirteen is opened by `window_event` over the length of its
    /// match and handed back at the foot of it ([`enter`]), so a handler's
    /// milliseconds are the handler's and the dispatch around them stays
    /// [`Self::Event`]'s. Which one an event opens is
    /// [`crate::window_event_station`] — [`crate::AppEvent::station`]'s twin,
    /// one door over and born of the same finding.
    EventClose = 35,
    /// `Runtime::keyboard_input` — the ladder every press and release of a key
    /// in this window walks.
    ///
    /// The heaviest thing a key can do without leaving this process, and almost
    /// none of it is charged here: the search box's live scan
    /// ([`Self::SearchScan`]), the chrome rebuild ([`Self::Chrome`]) and the
    /// frame ([`Self::Present`]) all name themselves, so what is left against
    /// this label is the ladder that reached them.
    EventKey = 36,
    /// `Runtime::ime_input` — one composition event from the input method,
    /// which on Windows arrives on IMM32's own synchronous call.
    EventIme = 37,
    /// `WindowEvent::ModifiersChanged` — **the one door every modifier state in
    /// this process comes through** (M1-7, §8 Q9).
    ///
    /// Three statements long and not therefore cheap: the pointer's shape is
    /// re-decided from it and the key hint is told about it, and either can end
    /// in the chrome being rebuilt.
    EventModifiers = 38,
    /// `Runtime::pointer_moved` and `Runtime::pointer_left` — the hover road,
    /// walked once per pointer sample the platform delivers.
    ///
    /// **One station for two arms**, because they are the same lane read at its
    /// two ends and because the same thing makes either slow: a hit test over
    /// every seat in the window, and the rebuild a hover that changed something
    /// asks for.
    EventPointer = 39,
    /// `Runtime::mouse_input` — a button going down or coming up, and every
    /// verb a press can reach from a tab strip, a pane head, a files row, a
    /// card or a rendered page.
    EventMouse = 40,
    /// `Runtime::queue_wheel` — one notch being added to the burst.
    ///
    /// Its own station rather than [`Self::Wheel`]'s, which is `flush_wheel`
    /// and is where the burst is actually spent: one is an addition and the
    /// other is a scroll, so a hold that lands here has named a very short
    /// piece of code — which is worth being able to read rather than assume.
    EventWheel = 41,
    /// `Runtime::resized` — a rectangle from the platform, with the solve, the
    /// re-measure and the shell resizes that follow it.
    EventResize = 42,
    /// `Runtime::scale_factor_changed` — this window arriving on a display with
    /// a different scale, which re-measures the font and re-solves every pane.
    EventScale = 43,
    /// `Runtime::window_moved`, `Runtime::os_theme_changed`, and the frame a
    /// window that has been uncovered owes (GitHub issue #5).
    ///
    /// **One station for three arms**, on [`Self::Chrome`]'s reasoning: each is
    /// the platform telling this window something about *itself* rather than
    /// about a hand, and a reader who sees time here has the fact they need.
    /// The move is the suspicious one of the three — it is a synchronous call
    /// into WebView2, and therefore into another process, on every drag of a
    /// window that has a page in it.
    EventWindow = 44,
    /// `Runtime::redraw` — the whole of a frame this window was asked for.
    ///
    /// Its own station rather than [`Self::Present`]'s, which is
    /// `publish_frame_inner` and is entered inside it: the difference between
    /// the two is everything a redraw does before it composes, and the two are
    /// repaired differently.
    EventRedraw = 45,
    /// `WindowEvent::Focused` — the keyboard arriving at this window or leaving
    /// it.
    ///
    /// One station for both arms, because they are the same list of things
    /// being put down and picked up again. The arm that can be slow is the
    /// arriving one: a window that has been away re-reads its git surfaces and
    /// the preview files no kernel would speak for, which is the only thing
    /// either arm asks a disk.
    EventFocus = 46,
    /// `WindowEvent::DroppedFile` — one path a hand let go of over this window,
    /// added to the batch the turn boundary spends ([`Self::FileDrop`]).
    ///
    /// **`HoveredFile` and `HoveredFileCancelled` are deliberately not here.**
    /// A drag passing over this window changes nothing on the glass — the drop
    /// affordance the files column would need is a ruling nobody has made — so
    /// both kinds fall through the dispatcher's own catch-all and answer
    /// [`Self::EventOther`], which is exactly what that label is for.
    EventFileDrop = 49,
    /// Every kind `window_event`'s match ends in `_ => Ok(())` for.
    ///
    /// **The label says `other` rather than naming them**, because the set is
    /// winit's and grows with it: an event this window does nothing for has
    /// cost a lookup and a comparison, and thirty stations that can never be
    /// the answer would bury the twelve that can. Time against this label is
    /// the dispatch itself — and, since that is very nearly impossible, a kind
    /// this window has started answering without being given a name.
    EventOther = 47,
    /// CPU-side frame composition and command encoding. The surface acquire
    /// sits between two intervals bearing this name, and the ledger adds them.
    RenderCompose = 50,
    /// `wgpu::Surface::get_current_texture`.
    SurfaceAcquire = 51,
    /// `wgpu::Queue::submit` after the command buffer has been finished.
    QueueSubmit = 52,
    /// `wgpu::Queue::present`, for a swapchain frame.
    SwapchainPresent = 53,
    /// winit's `Window::set_ime_cursor_area` platform call.
    ImeCursorArea = 54,
    /// Folio's platform system-caret update for the input method.
    ImeSystemCaret = 55,
    /// `bt_pty::OutputRing::try_pop`, through `read_output_slice`.
    DrainRingRead = 56,
    /// `DualPlaneSession::feed_at`: parse bytes and apply terminal events.
    DrainFeed = 57,
    /// Replies taken from the terminal actor and handed to bt-pty's writer.
    DrainReplies = 58,
    /// `DualPlaneSession::end_feed_turn`: settle the complete sliced feed.
    DrainSettle = 59,
    /// Interpret and publish the outcomes accumulated from all drained panes.
    DrainOutcomes = 60,
    /// The visible-artifact detection and scheduling pass over a projected frame.
    DetectionPass = 61,
    /// `Runtime::trace_drain` offering its line to the bounded trace sink.
    DrainTrace = 62,
    /// Decide whether terminal output publishes now or remains deferred.
    DrainPublish = 63,
    /// DirectComposition's update of the swapchain-covered rectangle.
    CompositorSize = 64,
    /// DirectComposition's `Commit`, which publishes the presented swapchain.
    CompositorCommit = 65,
    ImeEnabled = 66,
    ImePreedit = 67,
    ImeCommit = 68,
    ImeDisabled = 69,
    EventMoved = 70,
    EventTheme = 71,
    EventOccluded = 72,
    EventCursorLeft = 73,
    ImeAllowed = 74,
    ImeCaretDestroy = 75,
    PtyInput = 76,
    ClipboardWrite = 77,
    EventLookup = 78,
    EventSettleApplication = 79,
    EventRestore = 80,
    EventOpen = 81,
    EventQuit = 82,
    EventShut = 83,
    RedrawLayout = 84,
    RedrawProjection = 85,
    RedrawOverlay = 86,
    RedrawTables = 87,
    RedrawSignature = 88,
    RetainedPicture = 89,
    RedrawCommit = 90,
    PresentSeats = 91,
    /// `sample_window_place`, from `Runtime::observe_window_place` — the one
    /// reading of where the window is, taken at the head of every turn, at a
    /// window's birth and by an attention delivery between turns (ticket 48; was
    /// `DrainPlace`, same id and label, when the drain took it).
    Place = 92,
    /// `Window::has_focus`, the focus half of that reading (was `DrainFocus`).
    PlaceFocus = 93,
    DrainPane = 94,
    DrainPalette = 95,
    DrainRingStats = 96,
    DrainKeyboardFocus = 97,
    DrainMarks = 98,
    DrainAttention = 99,
    DrainRaiseAttention = 100,
    DrainGit = 101,
    /// `Window::set_title`, from `Runtime::flush_title` — the one write of the
    /// window's title, once a turn at most (ticket 49; was `DrainTitle`, same id
    /// and label, when the drain wrote it).
    WindowTitle = 102,
    DrainBegin = 103,
    DrainWake = 104,
    DrainWatermark = 105,
    SettingsWrite = 106,
    PreviewSave = 107,
    RenameDisk = 108,
    SharedLock = 109,
    EventGate = 110,
    ClockRaiseFirstRunIfDue = 111,
    ClockRaisePsreadlineInviteIfDue = 112,
    ClockAdvanceCursorBlinkIfDue = 113,
    ClockAdvanceRenameBlinkIfDue = 114,
    ClockAdvanceSchemeWatch = 115,
    ClockAdvanceStorageWatch = 116,
    ClockAdvancePreviewWatch = 117,
    ClockAdvanceFilesWatch = 118,
    ClockAdvanceTabPressIfDue = 119,
    ClockServicePictures = 120,
    ClockAdvanceStripAnimation = 121,
    ClockFinishSynchronizedUpdateIfDue = 122,
    ClockFinishPtyCoalesceIfDue = 123,
    ClockAdvanceGitWatch = 124,
    ClockSettleCompositionOwner = 125,
    ClockOfferImeCaret = 126,
    ClockFlushImeCursorArea = 127,
    ClockFinishResizeIfQuiescent = 128,
    ClockFinishPreviewScaleIfQuiet = 129,
    ClockAdvanceLiveMathIfDue = 130,
    ClockActivateHyperlinkHoverIfDue = 131,
    ClockActivatePeekIfDue = 132,
    ClockAdvanceChevrons = 133,
    ClockAdvancePaneMenu = 134,
    ClockAdvanceTermMenu = 135,
    ClockAdvanceTabMenu = 136,
    ClockAdvanceDragSpring = 137,
    ClockServiceDragAutoscroll = 138,
    ClockRefreshMathHoverAgainstThePicture = 139,
    ClockAdvanceMathToggleIfDue = 140,
    ClockAdvanceMathToolsIfDue = 141,
    ClockAdvanceLayoutPeekIfDue = 142,
    ClockAdvanceTooltipIfDue = 143,
    ClockNoteKeyHint = 144,
    ClockAdvanceKeyHintIfDue = 145,
    ClockNoteCardHint = 146,
    ClockAdvanceCardHint = 147,
    ClockAdvanceToasts = 148,
    ClockAdvanceCommandFlash = 149,
    ClockAdvanceCommandRails = 150,
    ClockAdvanceTerminalThumbs = 151,
    ClockAdvanceFilePeek = 152,
    ClockAdvanceFloat = 153,
    ClockRearmHoverIntents = 154,
    ClockAdvanceFootReveal = 155,
    ClockAdvancePageFootClocks = 156,
    ClockAdvancePreviewNotice = 157,
    ClockAdvancePreviewRefusal = 158,
    AtlasUpload = 159,
    TextShaping = 160,
    RenderLayout = 161,
    ImeCancel = 162,
    DrainChannel = 163,
    ClockFileDwell = 164,
    ClockFileClose = 165,
    ClockMathCopy = 166,
    ClockWebZoom = 167,
    ClockWebDialog = 168,
    EventActivation = 169,
    EventDestroyed = 170,
    EventHoveredFile = 171,
    EventHoverCancelled = 172,
    EventCursorEntered = 173,
    EventPinch = 174,
    EventPan = 175,
    EventDoubleTap = 176,
    EventRotation = 177,
    EventPressure = 178,
    EventAxis = 179,
    EventTouch = 180,
    RedrawTableSources = 181,
    RedrawSeatFrames = 182,
    RedrawValidate = 183,
    RedrawDispatch = 184,
    WindowRedraw = 185,
    WindowFocus = 186,
    WindowVisible = 187,
    WindowCursor = 188,
    KeybindingsWrite = 189,
    ProfilesWrite = 190,
    DrainTab = 191,
    ImeTrace = 192,
    DiagnosticWrite = 193,
    SurfaceConfigure = 194,
    /// `IsIconic` and the DWM cloak query, inside `sample_window_place`.
    PlaceHidden = 195,
    /// `bt_platform::window_is_exposed` — the exposure probe's `GetWindowRect`
    /// and hit tests, which ask whatever window is under each point.
    PlaceExposure = 196,
    /// `bt_platform::taskbar_is_auto_hidden` — `SHAppBarMessage`, a message to
    /// the shell's taskbar.
    PlaceTaskbar = 197,
    /// `TaskbarMirror::show`, the taskbar button's progress, from
    /// `Runtime::advance_strip_animation`.
    TaskbarMirror = 198,
    /// `WebSeat::start_environment`'s call into the host — the first page in
    /// the process spends `CreateCoreWebView2EnvironmentWithOptions` here (the
    /// loader, the runtime's discovery and the browser's launch request); every
    /// later page finds the process-wide environment cached and only queues its
    /// answer.
    ///
    /// **Born naming a stall that had already been reported** (ticket 43,
    /// D-64). The two holds of 2026-09-23 while a web preview opened —
    /// 4,099 ms and 2,884 ms — read `window_event 3979 ms` with every named
    /// child under 130 ms: the engine coming up had no word in this ledger, so
    /// the gesture that asked for it, the callback road it answered on and the
    /// burst that installed it were all one remainder. This station and the
    /// six after it are that remainder, named.
    WebEnvironment = 199,
    /// `WebHost::request_controller` —
    /// `CreateCoreWebView2CompositionController` on this window, a synchronous
    /// call whose controller arrives later by callback. Reached from
    /// [`Self::WebSpoke`] on the turn the environment's callback is read.
    WebController = 200,
    /// `Compositor::attach_web_visual` — the DirectComposition visual a
    /// controller that has just arrived is given, the first part of the
    /// `WebEffect::InstallEvents` burst.
    WebVisual = 201,
    /// `WebHost::install` — the controller taken, its settings said, every
    /// handler attached and its root visual target set, in one walk
    /// (`INSTALL_SEQUENCE`). The burst's second part.
    WebInstall = 202,
    /// `WebSeat::stand_on_the_floor` on the install turn — the new visual
    /// placed, its cover said, and the controller's scale, bounds and
    /// visibility told before anything navigates. The burst's third part; the
    /// same call on a frame's clock is [`Self::WebPlace`]'s.
    WebFloor = 203,
    /// `WebHost::navigate` — `ICoreWebView2::Navigate`, the first of which the
    /// install burst ends in, and every later one a seat is asked for.
    WebNavigate = 204,
    /// **Control is back with the platform's message pump inside a turn** —
    /// stamped at the foot of `window_event` and of `user_event`, so what the
    /// thread does between one of this program's handlers and the next is not
    /// charged to the handler that has already returned.
    ///
    /// **The callback road** (ticket 43). WebView2 is a single-threaded engine
    /// whose in-process half runs on this thread: its creation callbacks and
    /// whatever else it posts to itself are dispatched by the pump winit drives,
    /// between handlers. Before this station that time was charged to the last
    /// station standing, which after a window event was [`Self::Event`] — and
    /// that is the `window_event 3979 ms` the 2026-09-23 report could not
    /// divide. Time here is the platform's own loop and anything that runs on
    /// it: winit, the engine, a hook another program installed.
    Pump = 205,
    /// `settings::monospace_family_files`, from `apply_stored_terminal_font` —
    /// the face `settings.json` names, looked up by its name before the first
    /// grid is measured and whenever the face changes (ticket 50). Before the
    /// font list has landed this is one family asked of the system font
    /// collection; after, a row of that list.
    FontLookup = 206,
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
            Self::Parked => "parked",
            Self::Woken => "woken",
            Self::WebPlace => "sync_web_page",
            Self::WebRetire => "CoreWebView2Controller::Close",
            Self::WebOutcomes => "apply_web_outcomes",
            Self::Preview => "apply_preview_results",
            Self::Math => "apply_math_results",
            Self::Files => "apply_files_results",
            Self::Git => "apply_git_results",
            Self::Attention => "raise_attention",
            Self::Picture => "adopt_background_picture",
            Self::FileIndex => "apply_file_index_results",
            Self::Chrome => "refresh_chrome",
            Self::WebSpoke => "drive_web_page",
            Self::Settings => "settings_layout",
            Self::ClipboardRead => "clipboard read",
            Self::DpiSettle => "settle_dpi",
            Self::Pickers => "apply_pick_results",
            Self::PaneRows => "settle_pane_rows",
            Self::Watches => "watches",
            Self::SyncUpdate => "finish_synchronized_update",
            Self::Clocks => "window clocks",
            Self::Deadlines => "wake deadlines",
            Self::AppTurn => "application turn",
            Self::SearchScan => "search scan",
            Self::EventClose => "close_requested",
            Self::EventKey => "keyboard_input",
            Self::EventIme => "ime_input",
            Self::EventModifiers => "modifiers_changed",
            Self::EventPointer => "pointer_moved",
            Self::EventMouse => "mouse_input",
            Self::EventWheel => "queue_wheel",
            Self::EventResize => "resized",
            Self::EventScale => "scale_factor_changed",
            Self::EventWindow => "window state",
            Self::EventRedraw => "redraw",
            Self::EventFocus => "focused",
            Self::FileDrop => "flush_dropped_files",
            Self::EventFileDrop => "dropped_file",
            Self::EventOther => "window_event other",
            Self::RenderCompose => "redraw compose/encode",
            Self::SurfaceAcquire => "surface acquire",
            Self::QueueSubmit => "queue submit",
            Self::SwapchainPresent => "swapchain present",
            Self::ImeCursorArea => "IME set_cursor_area",
            Self::ImeSystemCaret => "IME system caret",
            Self::DrainRingRead => "PTY ring read",
            Self::DrainFeed => "terminal parse/feed",
            Self::DrainReplies => "PTY reply dispatch",
            Self::DrainSettle => "feed turn settle",
            Self::DrainOutcomes => "drain outcomes",
            Self::DetectionPass => "visible artifact detection",
            Self::DrainTrace => "trace sink offer",
            Self::DrainPublish => "drain publish decision",
            Self::CompositorSize => "compositor covered size",
            Self::CompositorCommit => "compositor commit",
            Self::ImeEnabled => "IME Enabled",
            Self::ImePreedit => "IME Preedit",
            Self::ImeCommit => "IME Commit",
            Self::ImeDisabled => "IME Disabled",
            Self::EventMoved => "moved",
            Self::EventTheme => "theme changed",
            Self::EventOccluded => "occluded",
            Self::EventCursorLeft => "cursor left",
            Self::ImeAllowed => "Window::set_ime_allowed",
            Self::ImeCaretDestroy => "ImeSystemCaret::destroy",
            Self::PtyInput => "PtySession::write input enqueue",
            Self::ClipboardWrite => "clipboard write",
            Self::EventLookup => "window runtime lookup",
            Self::EventSettleApplication => "settle_application_change",
            Self::EventRestore => "settle_restore_answer",
            Self::EventOpen => "open_pending_window",
            Self::EventQuit => "settle_quit",
            Self::EventShut => "close window",
            Self::RedrawLayout => "redraw layout",
            Self::RedrawProjection => "pane projection",
            Self::RedrawOverlay => "overlay build",
            Self::RedrawTables => "table paints",
            Self::RedrawSignature => "present signature",
            Self::RetainedPicture => "present_retained_picture",
            Self::RedrawCommit => "redraw bookkeeping",
            Self::PresentSeats => "present_seats_and_commit",
            Self::Place => "sample_window_place",
            Self::PlaceFocus => "Window::has_focus",
            Self::DrainPane => "drain pane",
            Self::DrainPalette => "terminal palette",
            Self::DrainRingStats => "PTY ring stats",
            Self::DrainKeyboardFocus => "terminal keyboard focus",
            Self::DrainMarks => "command marks and outcomes",
            Self::DrainAttention => "deliver_osc_attention",
            Self::DrainRaiseAttention => "drain raise_attention",
            Self::DrainGit => "reread_git_surfaces",
            Self::WindowTitle => "Window::set_title",
            Self::DrainBegin => "begin_feed_turn",
            Self::DrainWake => "PTY wake accept",
            Self::DrainWatermark => "command_marks_watermark",
            Self::SettingsWrite => "write_settings_atomic",
            Self::PreviewSave => "preview buffer save",
            Self::RenameDisk => "filesystem rename",
            Self::SharedLock => "shared lock acquisition",
            Self::EventGate => "window event gates",
            Self::ClockRaiseFirstRunIfDue => "first run",
            Self::ClockRaisePsreadlineInviteIfDue => "PSReadLine invite",
            Self::ClockAdvanceCursorBlinkIfDue => "shell caret",
            Self::ClockAdvanceRenameBlinkIfDue => "rename caret",
            Self::ClockAdvanceSchemeWatch => "schemes watch",
            Self::ClockAdvanceStorageWatch => "storage watch",
            Self::ClockAdvancePreviewWatch => "preview watch",
            Self::ClockAdvanceFilesWatch => "files watch",
            Self::ClockAdvanceTabPressIfDue => "tab press",
            Self::ClockServicePictures => "pictures",
            Self::ClockAdvanceStripAnimation => "strip animation",
            Self::ClockFinishSynchronizedUpdateIfDue => "synchronized update",
            Self::ClockFinishPtyCoalesceIfDue => "PTY coalesce",
            Self::ClockAdvanceGitWatch => "git watch",
            Self::ClockSettleCompositionOwner => "composition owner",
            Self::ClockOfferImeCaret => "IME caret offer",
            Self::ClockFlushImeCursorArea => "IME cursor",
            Self::ClockFinishResizeIfQuiescent => "resize finish",
            Self::ClockFinishPreviewScaleIfQuiet => "preview resample",
            Self::ClockAdvanceLiveMathIfDue => "live stability",
            Self::ClockActivateHyperlinkHoverIfDue => "hyperlink hover",
            Self::ClockActivatePeekIfDue => "peek hover",
            Self::ClockAdvanceChevrons => "chevrons",
            Self::ClockAdvancePaneMenu => "pane menu",
            Self::ClockAdvanceTermMenu => "terminal menu",
            Self::ClockAdvanceTabMenu => "tab menu",
            Self::ClockAdvanceDragSpring => "drag spring",
            Self::ClockServiceDragAutoscroll => "drag auto-scroll",
            Self::ClockRefreshMathHoverAgainstThePicture => "formula hover",
            Self::ClockAdvanceMathToggleIfDue => "formula toggle",
            Self::ClockAdvanceMathToolsIfDue => "formula tools",
            Self::ClockAdvanceLayoutPeekIfDue => "layout peek",
            Self::ClockAdvanceTooltipIfDue => "tooltip",
            Self::ClockNoteKeyHint => "key hint intent",
            Self::ClockAdvanceKeyHintIfDue => "key hint",
            Self::ClockNoteCardHint => "Cards hint intent",
            Self::ClockAdvanceCardHint => "Cards hint",
            Self::ClockAdvanceToasts => "toast",
            Self::ClockAdvanceCommandFlash => "command flash",
            Self::ClockAdvanceCommandRails => "command rails",
            Self::ClockAdvanceTerminalThumbs => "terminal thumbs",
            Self::ClockAdvanceFilePeek => "file peek",
            Self::ClockAdvanceFloat => "float",
            Self::ClockRearmHoverIntents => "hover intents",
            Self::ClockAdvanceFootReveal => "revealed foot",
            Self::ClockAdvancePageFootClocks => "page acknowledgements",
            Self::ClockAdvancePreviewNotice => "preview save notice",
            Self::ClockAdvancePreviewRefusal => "preview refusal",
            Self::AtlasUpload => "atlas upload",
            Self::TextShaping => "text shaping",
            Self::RenderLayout => "render layout",
            Self::ImeCancel => "IMM/TSF cancel_composition",
            Self::DrainChannel => "terminal reply channel receive",
            Self::ClockFileDwell => "file-peek dwell",
            Self::ClockFileClose => "file-peek close grace",
            Self::ClockMathCopy => "formula-copy acknowledgement",
            Self::ClockWebZoom => "web zoom acknowledgement",
            Self::ClockWebDialog => "web dialog acknowledgement",
            Self::EventActivation => "activation token",
            Self::EventDestroyed => "destroyed",
            Self::EventHoveredFile => "hovered file",
            Self::EventHoverCancelled => "hovered file cancelled",
            Self::EventCursorEntered => "cursor entered",
            Self::EventPinch => "pinch gesture",
            Self::EventPan => "pan gesture",
            Self::EventDoubleTap => "double tap gesture",
            Self::EventRotation => "rotation gesture",
            Self::EventPressure => "touchpad pressure",
            Self::EventAxis => "axis motion",
            Self::EventTouch => "touch",
            Self::RedrawTableSources => "table source collection",
            Self::RedrawSeatFrames => "seat frame assembly",
            Self::RedrawValidate => "redraw frame validation",
            Self::RedrawDispatch => "dispatch decoration tasks",
            Self::WindowRedraw => "Window::request_redraw",
            Self::WindowFocus => "Window::focus_window",
            Self::WindowVisible => "Window::set_visible",
            Self::WindowCursor => "Window::set_cursor",
            Self::KeybindingsWrite => "write_keybindings_atomic",
            Self::ProfilesWrite => "write_profiles_atomic",
            Self::DrainTab => "drain tab",
            Self::ImeTrace => "IME trace::Dump::line",
            Self::SurfaceConfigure => "surface configure",
            Self::DiagnosticWrite => "stderr diagnostic write",
            Self::PlaceHidden => "IsIconic + cloak",
            Self::PlaceExposure => "window_is_exposed",
            Self::PlaceTaskbar => "taskbar_is_auto_hidden",
            Self::TaskbarMirror => "TaskbarMirror::show",
            Self::WebEnvironment => "request_environment",
            Self::WebController => "request_controller",
            Self::WebVisual => "attach_web_visual",
            Self::WebInstall => "WebHost::install",
            Self::WebFloor => "stand_on_the_floor",
            Self::WebNavigate => "WebHost::navigate",
            Self::Pump => "message pump",
            Self::FontLookup => "font family lookup",
        }
    }

    /// This station's slot in a hold's ledger — the `repr`, read as an index.
    ///
    /// The same byte the atomic carries, so the ledger and the station can never
    /// be indexed by two different numbers.
    #[must_use]
    fn slot(self) -> usize {
        self as u8 as usize
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
            10 => Self::Parked,
            11 => Self::Woken,
            12 => Self::WebPlace,
            13 => Self::WebRetire,
            14 => Self::WebOutcomes,
            15 => Self::Preview,
            16 => Self::Math,
            17 => Self::Files,
            18 => Self::Git,
            19 => Self::Attention,
            20 => Self::Picture,
            21 => Self::FileIndex,
            22 => Self::Chrome,
            23 => Self::WebSpoke,
            24 => Self::Settings,
            25 => Self::ClipboardRead,
            26 => Self::DpiSettle,
            27 => Self::Pickers,
            28 => Self::PaneRows,
            29 => Self::Watches,
            30 => Self::SyncUpdate,
            31 => Self::Clocks,
            32 => Self::Deadlines,
            33 => Self::AppTurn,
            34 => Self::SearchScan,
            35 => Self::EventClose,
            36 => Self::EventKey,
            37 => Self::EventIme,
            38 => Self::EventModifiers,
            39 => Self::EventPointer,
            40 => Self::EventMouse,
            41 => Self::EventWheel,
            42 => Self::EventResize,
            43 => Self::EventScale,
            44 => Self::EventWindow,
            45 => Self::EventRedraw,
            46 => Self::EventFocus,
            47 => Self::EventOther,
            48 => Self::FileDrop,
            49 => Self::EventFileDrop,
            50 => Self::RenderCompose,
            51 => Self::SurfaceAcquire,
            52 => Self::QueueSubmit,
            53 => Self::SwapchainPresent,
            54 => Self::ImeCursorArea,
            55 => Self::ImeSystemCaret,
            56 => Self::DrainRingRead,
            57 => Self::DrainFeed,
            58 => Self::DrainReplies,
            59 => Self::DrainSettle,
            60 => Self::DrainOutcomes,
            61 => Self::DetectionPass,
            62 => Self::DrainTrace,
            63 => Self::DrainPublish,
            64 => Self::CompositorSize,
            65 => Self::CompositorCommit,
            66 => Self::ImeEnabled,
            67 => Self::ImePreedit,
            68 => Self::ImeCommit,
            69 => Self::ImeDisabled,
            70 => Self::EventMoved,
            71 => Self::EventTheme,
            72 => Self::EventOccluded,
            73 => Self::EventCursorLeft,
            74 => Self::ImeAllowed,
            75 => Self::ImeCaretDestroy,
            76 => Self::PtyInput,
            77 => Self::ClipboardWrite,
            78 => Self::EventLookup,
            79 => Self::EventSettleApplication,
            80 => Self::EventRestore,
            81 => Self::EventOpen,
            82 => Self::EventQuit,
            83 => Self::EventShut,
            84 => Self::RedrawLayout,
            85 => Self::RedrawProjection,
            86 => Self::RedrawOverlay,
            87 => Self::RedrawTables,
            88 => Self::RedrawSignature,
            89 => Self::RetainedPicture,
            90 => Self::RedrawCommit,
            91 => Self::PresentSeats,
            92 => Self::Place,
            93 => Self::PlaceFocus,
            94 => Self::DrainPane,
            95 => Self::DrainPalette,
            96 => Self::DrainRingStats,
            97 => Self::DrainKeyboardFocus,
            98 => Self::DrainMarks,
            99 => Self::DrainAttention,
            100 => Self::DrainRaiseAttention,
            101 => Self::DrainGit,
            102 => Self::WindowTitle,
            103 => Self::DrainBegin,
            104 => Self::DrainWake,
            105 => Self::DrainWatermark,
            106 => Self::SettingsWrite,
            107 => Self::PreviewSave,
            108 => Self::RenameDisk,
            109 => Self::SharedLock,
            110 => Self::EventGate,
            111 => Self::ClockRaiseFirstRunIfDue,
            112 => Self::ClockRaisePsreadlineInviteIfDue,
            113 => Self::ClockAdvanceCursorBlinkIfDue,
            114 => Self::ClockAdvanceRenameBlinkIfDue,
            115 => Self::ClockAdvanceSchemeWatch,
            116 => Self::ClockAdvanceStorageWatch,
            117 => Self::ClockAdvancePreviewWatch,
            118 => Self::ClockAdvanceFilesWatch,
            119 => Self::ClockAdvanceTabPressIfDue,
            120 => Self::ClockServicePictures,
            121 => Self::ClockAdvanceStripAnimation,
            122 => Self::ClockFinishSynchronizedUpdateIfDue,
            123 => Self::ClockFinishPtyCoalesceIfDue,
            124 => Self::ClockAdvanceGitWatch,
            125 => Self::ClockSettleCompositionOwner,
            126 => Self::ClockOfferImeCaret,
            127 => Self::ClockFlushImeCursorArea,
            128 => Self::ClockFinishResizeIfQuiescent,
            129 => Self::ClockFinishPreviewScaleIfQuiet,
            130 => Self::ClockAdvanceLiveMathIfDue,
            131 => Self::ClockActivateHyperlinkHoverIfDue,
            132 => Self::ClockActivatePeekIfDue,
            133 => Self::ClockAdvanceChevrons,
            134 => Self::ClockAdvancePaneMenu,
            135 => Self::ClockAdvanceTermMenu,
            136 => Self::ClockAdvanceTabMenu,
            137 => Self::ClockAdvanceDragSpring,
            138 => Self::ClockServiceDragAutoscroll,
            139 => Self::ClockRefreshMathHoverAgainstThePicture,
            140 => Self::ClockAdvanceMathToggleIfDue,
            141 => Self::ClockAdvanceMathToolsIfDue,
            142 => Self::ClockAdvanceLayoutPeekIfDue,
            143 => Self::ClockAdvanceTooltipIfDue,
            144 => Self::ClockNoteKeyHint,
            145 => Self::ClockAdvanceKeyHintIfDue,
            146 => Self::ClockNoteCardHint,
            147 => Self::ClockAdvanceCardHint,
            148 => Self::ClockAdvanceToasts,
            149 => Self::ClockAdvanceCommandFlash,
            150 => Self::ClockAdvanceCommandRails,
            151 => Self::ClockAdvanceTerminalThumbs,
            152 => Self::ClockAdvanceFilePeek,
            153 => Self::ClockAdvanceFloat,
            154 => Self::ClockRearmHoverIntents,
            155 => Self::ClockAdvanceFootReveal,
            156 => Self::ClockAdvancePageFootClocks,
            157 => Self::ClockAdvancePreviewNotice,
            158 => Self::ClockAdvancePreviewRefusal,
            159 => Self::AtlasUpload,
            160 => Self::TextShaping,
            161 => Self::RenderLayout,

            162 => Self::ImeCancel,
            163 => Self::DrainChannel,
            164 => Self::ClockFileDwell,
            165 => Self::ClockFileClose,
            166 => Self::ClockMathCopy,
            167 => Self::ClockWebZoom,
            168 => Self::ClockWebDialog,
            169 => Self::EventActivation,
            170 => Self::EventDestroyed,
            171 => Self::EventHoveredFile,
            172 => Self::EventHoverCancelled,
            173 => Self::EventCursorEntered,
            174 => Self::EventPinch,
            175 => Self::EventPan,
            176 => Self::EventDoubleTap,
            177 => Self::EventRotation,
            178 => Self::EventPressure,
            179 => Self::EventAxis,
            180 => Self::EventTouch,
            181 => Self::RedrawTableSources,
            182 => Self::RedrawSeatFrames,
            183 => Self::RedrawValidate,
            184 => Self::RedrawDispatch,
            185 => Self::WindowRedraw,
            186 => Self::WindowFocus,
            187 => Self::WindowVisible,
            188 => Self::WindowCursor,
            189 => Self::KeybindingsWrite,
            190 => Self::ProfilesWrite,
            191 => Self::DrainTab,
            192 => Self::ImeTrace,
            193 => Self::DiagnosticWrite,
            194 => Self::SurfaceConfigure,
            195 => Self::PlaceHidden,
            196 => Self::PlaceExposure,
            197 => Self::PlaceTaskbar,
            198 => Self::TaskbarMirror,
            199 => Self::WebEnvironment,
            200 => Self::WebController,
            201 => Self::WebVisual,
            202 => Self::WebInstall,
            203 => Self::WebFloor,
            204 => Self::WebNavigate,
            205 => Self::Pump,
            206 => Self::FontLookup,
            _ => Self::Starting,
        }
    }
}

impl fmt::Display for Station {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// **What the window thread told the platform about when to expect it back.**
///
/// The one fact that separates an idle terminal from a wedged one, and the
/// window thread is the only place it exists: `about_to_wait` decides the
/// `ControlFlow` and therefore knows, at the moment it hands control over,
/// whether anything is owed to it at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Park {
    /// Not parked. The thread holds control and the loop is expected to come
    /// round, so silence here is measured against the ordinary threshold.
    Running,
    /// Parked with a deadline (`ControlFlow::WaitUntil`), in heartbeat
    /// milliseconds. The platform owes a wake at that instant; silence well past
    /// it is a promise that was not kept.
    Until(u64),
    /// Parked with no deadline (`ControlFlow::Wait`). **Nothing is owed**, so no
    /// amount of silence here is a hang — this is what an idle window does, and
    /// reading it as a fault is the bug this enum exists to fix.
    Indefinite,
}

/// `Park::Running`, as the byte pattern the atomic holds.
///
/// Zero, so the state a fresh `Heartbeat` is born in is the state before the
/// first park — and so that clearing the parking is a store of a constant.
const PARK_RUNNING: u64 = 0;

/// `Park::Indefinite`, as the byte pattern the atomic holds.
///
/// `u64::MAX`, which is also a deadline no clock in this process reaches, so
/// the encoding degrades correctly in the one direction it could be misread:
/// an indefinite park mistaken for a deadline is a deadline that never passes.
const PARK_INDEFINITE: u64 = u64::MAX;

impl Park {
    fn to_bits(self) -> u64 {
        match self {
            Self::Running => PARK_RUNNING,
            // A deadline of exactly zero is unreachable — the origin is fixed
            // before the event loop exists — but the encoding says so rather
            // than assuming it.
            Self::Until(deadline) => deadline.max(1),
            Self::Indefinite => PARK_INDEFINITE,
        }
    }

    fn from_bits(bits: u64) -> Self {
        match bits {
            PARK_RUNNING => Self::Running,
            PARK_INDEFINITE => Self::Indefinite,
            deadline => Self::Until(deadline),
        }
    }
}

/// One reading of the window thread's pulse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pulse {
    /// Milliseconds from the heartbeat's origin to the loop's last turn.
    pub at_ms: u64,
    /// **How many turns the loop has taken.** A turning loop is alive, and the
    /// clock alone would not say so — a stopped thread's `at_ms` is a perfectly
    /// stable value, and a loop that spins without progressing would still be
    /// reading the clock. The counter only advances where the pump actually
    /// comes round.
    ///
    /// What it is *not* is the whole of liveness: a loop that has stopped
    /// turning because it was told to wait is doing its job. See [`Pulse::park`].
    pub turn: u64,
    pub station: Station,
    /// Where the thread is between turns: holding control, or parked, and if
    /// parked, until when.
    pub park: Park,
}

/// **What the memory manager did to this process across one hold** — the other
/// half of the account, and the half the stations cannot give.
///
/// A ledger that says `flush_wheel 1928 ms` names where the milliseconds were
/// spent and says nothing about *whose* they were. Two seconds inside one call
/// is either two seconds of this program's own work — which is repaired here —
/// or this program standing still while the operating system reads its working
/// set back in, which is not a fault in this program at all and is exactly what
/// the machine the fault is reported on does when it is carrying more committed
/// memory than it has RAM. The counters below are how a reader tells those
/// apart without being at the machine: faults climbing by tens of thousands
/// while the resident size climbs beside them is a second that belonged to the
/// memory manager, and a hold that spends one with both numbers flat spent it
/// here.
///
/// See [`bt_platform::mem`] for what "a fault" counts on each platform — the
/// two are not the same quantity and the difference is written down there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Paging {
    /// Page faults taken between the two ends of the hold.
    ///
    /// A difference and not a reading: the platform's counter is cumulative
    /// since the process started, and the only interesting number is how much
    /// of it belongs to this hold.
    pub faults: u64,
    /// The resident size in bytes as the hold opened.
    pub working_set_before: u64,
    /// The resident size in bytes as it closed. Printed beside the one above
    /// rather than as a difference, because the direction is not the point:
    /// **a working set that grew is a set being read back in, and one that
    /// shrank during a long hold is one being trimmed while the thread was
    /// held** — and a signed delta would print the same digit for two opposite
    /// stories.
    pub working_set_after: u64,
}

/// One binary megabyte, which is the megabyte a reader's Task Manager and
/// Activity Monitor both print.
const BYTES_PER_MEGABYTE: u64 = 1024 * 1024;

/// `bytes` as the megabytes the line prints, rounded to the nearest.
///
/// Rounded rather than truncated because the pair is read as a movement — 179
/// to 412 — and a truncation makes a set that grew by a megabyte and a half
/// look like one that grew by one.
#[must_use]
fn megabytes(bytes: u64) -> u64 {
    bytes.saturating_add(BYTES_PER_MEGABYTE / 2) / BYTES_PER_MEGABYTE
}

/// **One hold of the window thread that ran long, and where its time went.**
///
/// A hold is `woke` → `park`: everything between the platform handing this
/// thread control and this thread handing it back, events and turn together.
/// `spent_ms` is indexed by [`Station::slot`] and is the account of that hold —
/// what the ledger could not attribute to a named call shows up against the
/// station the thread was last in, which is the same honesty [`Station`] itself
/// carries.
///
/// `Copy` and plain integers, because the thread that fills it in is the one
/// this module exists to diagnose: nothing here allocates, and the formatting
/// happens on the watchdog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlowHold {
    /// The turn counter as it stood when control was handed back.
    pub turn: u64,
    /// How long control was held, measured end to end rather than summed —
    /// so a gap the ledger failed to attribute shows up as the line not adding
    /// up rather than as time that never existed.
    pub held_ms: u64,
    /// Milliseconds since this process's heartbeat origin.
    pub session_age_ms: u64,
    /// One-based count of slow-hold lines successfully queued in this run.
    pub stall_count: u64,
    /// Milliseconds per station, indexed by [`Station::slot`].
    pub spent_ms: [u64; STATION_COUNT],
    /// What the machine's memory manager did while the hold ran, when this
    /// platform counts it and both ends were sampled. See [`Paging`].
    pub paging: Option<Paging>,
    pub cpu_us: Option<u64>,
    detail: detail::Tree,
}

impl SlowHold {
    /// The one line this hold writes to the diagnostics log.
    ///
    /// Stations in order of what they cost, silent ones left out: the reader's
    /// question is *what took the time*, and a line that spelled out eleven
    /// zeroes to say "`advance_web_page`" would bury its own answer.
    ///
    /// Pure, and taking nothing but itself, so the shape of the line is a thing
    /// a test can state — [`crate::diagnostics::run_header`]'s own rule.
    #[must_use]
    pub fn line(&self) -> String {
        let mut spent: Vec<(Station, u64)> = self
            .spent_ms
            .iter()
            .enumerate()
            .filter(|(_, spent)| **spent > 0)
            .map(|(slot, spent)| {
                (
                    Station::from_byte(u8::try_from(slot).unwrap_or_default()),
                    *spent,
                )
            })
            .collect();
        // Longest first, and ties broken by the station's own order so that two
        // runs of the same program print the same line.
        spent.sort_by(|left, right| {
            right
                .1
                .cmp(&left.1)
                .then(left.0.slot().cmp(&right.0.slot()))
        });
        let where_ = if spent.is_empty() {
            String::from("no station held it")
        } else {
            spent
                .iter()
                .map(|(station, spent)| format!("{station} {spent} ms"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let where_ = self
            .detail
            .line()
            .filter(|line| !line.is_empty())
            .unwrap_or(where_);
        let mut line = format!(
            "Folio: the window thread held control for {} ms on turn {} — {where_}",
            self.held_ms, self.turn
        );
        if let Some(cpu_us) = self.cpu_us {
            line.push_str(&format!(
                " · thread CPU {}.{:03} ms / wall {} ms",
                cpu_us / 1000,
                cpu_us % 1000,
                self.held_ms
            ));
        }
        if self.detail.overflowed() {
            line.push_str(" · detail capacity exceeded");
        }
        // Appended, never interleaved, and behind a separator no station label
        // contains: every line this instrument has ever written keeps its
        // shape, and a reader who greps for `held control for` or for a station
        // name finds the same lines they found before the counters existed.
        if let Some(paging) = self.paging {
            line.push_str(&format!(
                " · faults +{}, working set {} → {} MB",
                paging.faults,
                megabytes(paging.working_set_before),
                megabytes(paging.working_set_after),
            ));
        }
        line.push_str(&format!(
            " · session age {}, stall #{}",
            seconds(self.session_age_ms),
            self.stall_count
        ));
        line
    }
}

/// The window thread's own three words, and the clock they are measured on.
///
/// Not a `Mutex` and not a channel: the writer is the thread this exists to
/// diagnose, so the write has to be something that cannot itself block, cannot
/// allocate, and cannot be what wedges. Three atomics is the whole of it.
#[derive(Debug)]
pub struct Heartbeat {
    detail: detail::Ledger,
    cpu_time: fn() -> Option<u64>,
    held_cpu: AtomicU64,
    origin: Instant,
    at_ms: AtomicU64,
    turn: AtomicU64,
    station: AtomicU8,
    /// [`Park`], encoded. A fourth atomic and a fourth relaxed store per turn —
    /// see [`Park::to_bits`] for the encoding and the module comment for why the
    /// facility is wrong without it.
    park: AtomicU64,
    /// **When the hold in progress began, plus one** — or zero while the thread
    /// is parked.
    ///
    /// Offset by one for [`Park::to_bits`]' reason, one field over: zero has to
    /// mean *no hold is open*, which is what makes [`Self::close_hold`]
    /// idempotent on the paths that park twice, and a hold that opened on
    /// millisecond zero of the run is a real hold rather than the absence of
    /// one. It is a real case and not a hypothetical — the origin is taken at
    /// the first touch of the heartbeat, and the loop's first wake can land
    /// inside that same millisecond.
    held_since_ms: AtomicU64,
    /// When the station in progress was entered.
    station_since_ms: AtomicU64,
    /// The hold in progress, by station. See [`SlowHold::spent_ms`].
    spent_ms: [AtomicU64; STATION_COUNT],
    /// Holds that ran past [`SLOW_HOLD_THRESHOLD`], waiting to be written.
    ///
    /// A `Mutex` the window thread only ever `try_lock`s. The rule the rest of
    /// this struct keeps — the writer is the thread this exists to diagnose, so
    /// the write cannot itself block — is kept here by never waiting: a hold
    /// that arrives while the watchdog is draining is counted rather than
    /// waited for.
    slow: Mutex<Vec<SlowHold>>,
    /// Slow holds that found the queue full or busy.
    slow_dropped: AtomicU64,
    /// Slow-hold lines successfully admitted to [`Self::slow`].
    slow_reported: AtomicU64,
    /// **Where the two footprint readings come from.**
    ///
    /// A function pointer rather than a direct call to
    /// [`bt_platform::mem::footprint`], for the reason the four `_at` verbs take
    /// a clock: the arithmetic worth pinning is *this* module's — which sample
    /// is taken when, and what the line says about the pair — and a test that
    /// could only get numbers out of the real memory manager could state none of
    /// it. A pointer and not a boxed closure, so the field costs one word, the
    /// struct keeps its derived `Debug`, and the call is the same indirect jump
    /// on the window thread as a direct one through a `LazyLock`.
    footprint: fn() -> Option<Footprint>,
    /// The process's fault count as the hold in progress opened.
    held_faults: AtomicU64,
    /// Its resident size at that same instant, in bytes.
    held_working_set: AtomicU64,
    /// Whether the two above were actually taken. A flag rather than a sentinel
    /// in either number, because both of them have legitimate values everywhere
    /// in their range and a platform with no arm answers nothing at all.
    held_footprint: AtomicBool,
    /// When the cached opening footprint was sampled, plus one; zero means no
    /// sample has yet been attempted.
    footprint_sampled_at_ms: AtomicU64,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self::new()
    }
}

impl Heartbeat {
    #[must_use]
    pub fn new() -> Self {
        let mut heart = Self::sampling(bt_platform::mem::footprint);
        heart.cpu_time = armed_thread_cpu_us;
        heart
    }

    /// [`Self::new`], with the footprint sampler named. See [`Self::footprint`].
    #[must_use]
    fn sampling(footprint: fn() -> Option<Footprint>) -> Self {
        Self {
            footprint,
            detail: detail::Ledger::new(),
            cpu_time: || None,
            held_cpu: AtomicU64::new(0),
            held_faults: AtomicU64::new(0),
            held_working_set: AtomicU64::new(0),
            held_footprint: AtomicBool::new(false),
            footprint_sampled_at_ms: AtomicU64::new(0),
            origin: Instant::now(),
            at_ms: AtomicU64::new(0),
            turn: AtomicU64::new(0),
            station: AtomicU8::new(Station::Starting as u8),
            park: AtomicU64::new(PARK_RUNNING),
            held_since_ms: AtomicU64::new(0),
            station_since_ms: AtomicU64::new(0),
            spent_ms: std::array::from_fn(|_| AtomicU64::new(0)),
            slow: Mutex::new(Vec::new()),
            slow_dropped: AtomicU64::new(0),
            slow_reported: AtomicU64::new(0),
        }
    }

    /// Milliseconds since this heartbeat started. Monotonic.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// `instant` on this heartbeat's own clock.
    ///
    /// How a `ControlFlow::WaitUntil` deadline — an `Instant` the loop computed
    /// from its own clocks — becomes a number the watchdog can compare against
    /// [`Self::now_ms`]. Saturating on both ends: a deadline already in the past
    /// answers a small number, which reads as "overdue" and is exactly right.
    #[must_use]
    pub fn ms_at(&self, instant: Instant) -> u64 {
        u64::try_from(instant.saturating_duration_since(self.origin).as_millis())
            .unwrap_or(u64::MAX)
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
        self.beat_at(self.now_ms());
    }

    /// [`Self::beat`] on a clock the caller holds. See [`Self::park_at`].
    pub fn beat_at(&self, now_ms: u64) {
        // A turn reached without a wake before it — the run's first, and every
        // turn on a platform that delivers no `StartCause` — still opens a hold,
        // or the ledger would measure this one from an origin belonging to a
        // hold that has already been written down.
        if self.held_since_ms.load(Ordering::Relaxed) == 0 {
            self.open_hold(now_ms);
        }
        self.move_to(Station::Wait, now_ms);
        self.at_ms.store(now_ms, Ordering::Relaxed);
        self.park.store(PARK_RUNNING, Ordering::Relaxed);
        self.turn.fetch_add(1, Ordering::Release);
    }

    /// **Close the station in progress and open `station`'s**, charging the
    /// milliseconds between them to the one that is ending.
    ///
    /// The one place the ledger is written. It charges the station the thread
    /// was *in*, which is what makes untagged code between two stations show up
    /// against the last named call rather than vanishing — [`Station`]'s own
    /// rule, now counted instead of merely labelled.
    fn move_to(&self, station: Station, now_ms: u64) {
        self.charge(station, now_ms);
        self.detail.at(station);
    }

    fn charge(&self, station: Station, now_ms: u64) {
        let leaving = Station::from_byte(self.station.load(Ordering::Relaxed));
        let since = self.station_since_ms.load(Ordering::Relaxed);
        self.detail.charge(now_ms.saturating_sub(since));
        self.spent_ms[leaving.slot()].fetch_add(now_ms.saturating_sub(since), Ordering::Relaxed);
        self.station_since_ms.store(now_ms, Ordering::Relaxed);
        self.station.store(station as u8, Ordering::Relaxed);
    }

    /// **A hold begins**: the ledger is emptied, the coarse footprint baseline
    /// refreshed if needed and the clock started.
    fn open_hold(&self, now_ms: u64) {
        self.detail.clear();
        self.held_cpu.store(
            (self.cpu_time)().map_or(0, |us| us.saturating_add(1)),
            Ordering::Relaxed,
        );
        for spent in &self.spent_ms {
            spent.store(0, Ordering::Relaxed);
        }
        self.open_footprint(now_ms);
        self.station_since_ms.store(now_ms, Ordering::Relaxed);
        self.held_since_ms
            .store(now_ms.saturating_add(1), Ordering::Relaxed);
    }

    /// **Refresh the coarse opening baseline when it is old.**
    ///
    /// The tempting design is to sample only the holds that turn out to be
    /// reported, and it cannot be built: *slow* is a fact about a hold that is
    /// not known until the hold ends, and the baseline has to have been taken
    /// before it began. The two ways of learning about a long hold while it is
    /// still running both fail on the case the instrument exists for:
    ///
    /// * **The window thread noticing at a station** — the crossing could be
    ///   tested for free inside [`Self::move_to`], which already holds the
    ///   clock. But a hold that is *one long call* passes no station while it
    ///   runs; a 1928 ms `flush_wheel` would arm the sampler on its way out, the
    ///   baseline would be read after the paging was over, and the line would
    ///   print `faults +0` on precisely the hold it was built to explain.
    /// * **The watchdog noticing from outside** — it wakes every
    ///   [`WATCH_INTERVAL`], four times longer than [`SLOW_HOLD_THRESHOLD`], and
    ///   a fault counter it reads is a fact about the moment *it* woke rather
    ///   than about either end of somebody else's hold.
    ///
    /// The opening sample is therefore allowed to precede its hold by at most
    /// [`FOOTPRINT_SAMPLE_INTERVAL`], half the threshold that makes a hold worth
    /// reporting. That bounded attribution error is preferable to a kernel call
    /// on every harmless run-loop iteration. The other end,
    /// [`Self::close_footprint`], is on the reporting path alone and is reached
    /// by roughly none of them.
    fn open_footprint(&self, now_ms: u64) {
        let sampled = self.footprint_sampled_at_ms.load(Ordering::Relaxed);
        let age = now_ms.saturating_sub(sampled.saturating_sub(1));
        if sampled != 0
            && age < u64::try_from(FOOTPRINT_SAMPLE_INTERVAL.as_millis()).unwrap_or(u64::MAX)
        {
            return;
        }
        if let Some(footprint) = (self.footprint)() {
            self.held_faults.store(footprint.faults, Ordering::Relaxed);
            self.held_working_set
                .store(footprint.working_set_bytes, Ordering::Relaxed);
            self.held_footprint.store(true, Ordering::Relaxed);
        } else {
            self.held_footprint.store(false, Ordering::Relaxed);
        }
        self.footprint_sampled_at_ms
            .store(now_ms.saturating_add(1), Ordering::Relaxed);
    }

    /// **The second sample, taken only for a hold that is already going to be
    /// written down.**
    ///
    /// `None` when this platform counts nothing, when the opening sample was
    /// refused, or when this one is — a line that named one end of a movement
    /// would be worse than a line that names neither.
    #[must_use]
    fn close_footprint(&self) -> Option<Paging> {
        if !self.held_footprint.load(Ordering::Relaxed) {
            return None;
        }
        let closing = (self.footprint)()?;
        Some(Paging {
            // Saturating, which is the harmless direction: the only way this
            // subtraction can go negative is the 32-bit Windows counter having
            // wrapped mid-hold, and a `+0` reads as "nothing to see here" while
            // a wrapped difference would read as four billion faults.
            faults: closing
                .faults
                .saturating_sub(self.held_faults.load(Ordering::Relaxed)),
            working_set_before: self.held_working_set.load(Ordering::Relaxed),
            working_set_after: closing.working_set_bytes,
        })
    }

    /// **A hold ends**, and if it ran long it is queued for the watchdog.
    ///
    /// Idempotent by way of the swap: a second park with no wake between them
    /// finds no hold open and has nothing to say, which is what keeps the two
    /// parkings a failed turn can leave from being counted as two holds.
    fn close_hold(&self, now_ms: u64) {
        let began = self.held_since_ms.swap(0, Ordering::Relaxed);
        if began == 0 {
            return;
        }
        let held_ms = now_ms.saturating_sub(began - 1);
        let mut spent_ms = [0; STATION_COUNT];
        for (slot, cell) in spent_ms.iter_mut().zip(&self.spent_ms) {
            *slot = cell.swap(0, Ordering::Relaxed);
        }
        if held_ms < slow_hold_threshold_ms() {
            return;
        }
        // Below the threshold this line is never reached, which is the whole of
        // what keeps the second system call off the ordinary turn.
        let paging = self.close_footprint();
        // `try_lock` and never `lock`: see [`Self::slow`].
        if let Ok(mut queue) = self.slow.try_lock()
            && queue.len() < SLOW_HOLDS_KEPT
        {
            let stall_count = self.slow_reported.fetch_add(1, Ordering::Relaxed) + 1;
            let baseline = self.held_cpu.load(Ordering::Relaxed);
            let cpu_us = (baseline != 0)
                .then(|| (self.cpu_time)())
                .flatten()
                .and_then(|closing| closing.checked_sub(baseline - 1));
            queue.push(SlowHold {
                cpu_us,
                detail: self.detail.snapshot(),
                turn: self.turn.load(Ordering::Relaxed),
                held_ms,
                session_age_ms: now_ms,
                stall_count,
                spent_ms,
                paging,
            });
        } else {
            self.slow_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// **Take every slow hold recorded since the last ask**, and how many were
    /// dropped. Called from the watchdog thread.
    ///
    /// Answers nothing at all rather than waiting when the window thread has the
    /// queue: the next poll is two seconds away and the records are not going
    /// anywhere, while a watchdog that blocked on the thread it is watching
    /// would be the one thing this facility must never be.
    #[must_use]
    pub fn take_slow_holds(&self) -> (Vec<SlowHold>, u64) {
        let Ok(mut queue) = self.slow.try_lock() else {
            return (Vec::new(), 0);
        };
        (
            std::mem::take(&mut *queue),
            self.slow_dropped.swap(0, Ordering::Relaxed),
        )
    }

    /// **The window thread entered a named call.** Clocked exclusive accounting.
    ///
    /// No ordering, because none is owed: this is a hint about a thread that is
    /// still running, and a watchdog that reads a station one instruction stale
    /// has read a true fact about one instruction ago.
    ///
    /// The parking is cleared here as well as in [`Self::beat`], and that is the
    /// invariant the whole facility rests on: **arriving anywhere named means
    /// holding control**. Without it a wake that reaches `window_event` without
    /// completing a turn would still be wearing the previous turn's park, and a
    /// wedge inside that event would be excused by a deadline that had nothing
    /// to do with it.
    pub fn at(&self, station: Station) {
        self.at_station(station, self.now_ms());
    }

    /// [`Self::at`] on a clock the caller holds. See [`Self::park_at`].
    pub fn at_station(&self, station: Station, now_ms: u64) {
        self.move_to(station, now_ms);
        self.park.store(PARK_RUNNING, Ordering::Relaxed);
    }

    /// **The window thread is handing control back to the platform**, and this
    /// is when it means to be back.
    ///
    /// Called once per turn, last — after the loop has decided its
    /// `ControlFlow`, because that decision *is* this fact. The station is
    /// stamped before the parking so that a watchdog which reads the two in
    /// either order never sees a park without the station that explains it.
    pub fn park(&self, park: Park) {
        self.park_at(park, self.now_ms());
    }

    /// [`Self::park`] on a clock the caller holds.
    ///
    /// The four verbs are split this way for [`crate::diagnostics::run_header`]'s
    /// reason: the ledger's arithmetic is the thing worth pinning, and an
    /// instrument whose only clock is the real one can be exercised by a test
    /// only by sleeping — which is how a facility ends up with a test that
    /// passes on a fast machine.
    pub fn park_at(&self, park: Park, now_ms: u64) {
        self.move_to(Station::Parked, now_ms);
        self.close_hold(now_ms);
        self.park.store(park.to_bits(), Ordering::Relaxed);
    }

    /// **The platform woke the thread.** The park is over; the turn has not
    /// happened yet.
    ///
    /// The other end of [`Self::park`], and it has to be its own call because a
    /// wake does not always reach a turn: an event is delivered first, and a
    /// thread that wedges inside that event is a thread holding control, not a
    /// thread parked.
    pub fn woke(&self) {
        self.woke_at(self.now_ms());
    }

    /// [`Self::woke`] on a clock the caller holds. See [`Self::park_at`].
    ///
    /// This is where a hold opens, because this is where the platform hands
    /// control over — everything the thread then does, events included, belongs
    /// to one hold and is accounted for together. A stall inside `window_event`
    /// is exactly the shape the report of 2026-08-30 describes, and a ledger
    /// that only opened at `about_to_wait` would have nothing to say about it.
    pub fn woke_at(&self, now_ms: u64) {
        self.open_hold(now_ms);
        self.station.store(Station::Woken as u8, Ordering::Relaxed);
        self.detail.at(Station::Woken);
        self.park.store(PARK_RUNNING, Ordering::Relaxed);
    }

    /// Read all four, coherently. Safe from any thread.
    ///
    /// The turn counter is read first and with `Acquire`, which pairs with the
    /// `Release` in [`Self::beat`]. Everything after it is relaxed and may be
    /// one turn newer than the counter — and each way that can go is the safe
    /// way: a park read fresher than its turn is a *future* deadline, which
    /// suspects nothing, and a park cleared but not yet observed costs one poll
    /// interval of patience, never a report.
    #[must_use]
    pub fn sample(&self) -> Pulse {
        let turn = self.turn.load(Ordering::Acquire);
        Pulse {
            at_ms: self.at_ms.load(Ordering::Relaxed),
            turn,
            station: Station::from_byte(self.station.load(Ordering::Relaxed)),
            park: Park::from_bits(self.park.load(Ordering::Relaxed)),
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
static CPU_ARMED: AtomicBool = AtomicBool::new(false);

fn armed_thread_cpu_us() -> Option<u64> {
    CPU_ARMED
        .load(Ordering::Relaxed)
        .then(bt_platform::mem::thread_cpu_us)
        .flatten()
}

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
pub fn at(location: impl Into<Location>) {
    match location.into() {
        Location::Station(station) => HEARTBEAT.at(station),
        Location::Resume {
            station,
            node,
            scope,
        } => HEARTBEAT.resume_at(station, node, scope, HEARTBEAT.now_ms()),
    }
}

/// Opaque return address for an exclusive scope, including its call-tree path.
#[derive(Clone, Copy)]
pub enum Location {
    Station(Station),
    Resume {
        station: Station,
        node: usize,
        scope: usize,
    },
}

impl From<Station> for Location {
    fn from(station: Station) -> Self {
        Self::Station(station)
    }
}

impl Heartbeat {
    fn enter_at(&self, station: Station, pane: u64, now: u64) -> Location {
        let parent = self.detail.current();
        let scope = self.detail.scope();
        let previous = Station::from_byte(self.station.load(Ordering::Relaxed));
        self.charge(station, now);
        self.park.store(PARK_RUNNING, Ordering::Relaxed);
        self.detail.enter(station, parent, pane);
        self.detail.set_scope(self.detail.current());
        Location::Resume {
            station: previous,
            node: parent,
            scope,
        }
    }

    fn resume_at(&self, station: Station, node: usize, scope: usize, now: u64) {
        self.charge(station, now);
        self.park.store(PARK_RUNNING, Ordering::Relaxed);
        self.detail.restore(node);
        self.detail.set_scope(scope);
    }
}

/// Attribute subsequent present stations without formatting on the window thread.
pub fn present_attempt(window: u64, generation: u64, sequence: u64) {
    HEARTBEAT.detail.attempt([window, generation, sequence]);
}

pub fn present_generation(generation: u64) {
    HEARTBEAT.detail.generation(generation);
}
pub fn end_present_attempt() {
    HEARTBEAT.detail.attempt([0; 3]);
}

fn present_progress(station: Station) -> &'static str {
    match station {
        Station::SurfaceConfigure => "in_progress:configure",
        Station::SurfaceAcquire => "in_progress:acquire",
        Station::QueueSubmit => "in_progress:submit",
        Station::SwapchainPresent => "in_progress:present",
        Station::CompositorSize | Station::CompositorCommit => "in_progress:commit",
        Station::RenderCompose
        | Station::TextShaping
        | Station::AtlasUpload
        | Station::RenderLayout => "in_progress:encode",
        Station::RedrawCommit => "in_progress:acknowledge",
        _ => "in_progress:prepare",
    }
}

/// Numeric evidence only; input contents are never recorded.
pub fn counters(bytes: usize, accepted: usize, count: usize) {
    HEARTBEAT.detail.counters(bytes, accepted, count);
}

/// A renderer callback changes sibling phases inside one presentation scope.
pub fn phase(station: Station) {
    HEARTBEAT.charge(station, HEARTBEAT.now_ms());
    HEARTBEAT.park.store(PARK_RUNNING, Ordering::Relaxed);
    HEARTBEAT.detail.phase(station);
}

/// Enter a pane scope; the ID is part of the fixed ledger key.
pub fn enter_pane(station: Station, pane: u64) -> Location {
    HEARTBEAT.enter_at(station, pane.saturating_add(1), HEARTBEAT.now_ms())
}

/// **Enter `station`, and answer the one being left** so the caller can put it
/// back on the way out.
///
/// For a station that stands *inside* another station's function — a single
/// call worth timing on its own, where charging the rest of the enclosing
/// function to it afterwards would be a lie the ledger tells quietly. The
/// caller pairs this with [`at`]:
///
/// ```ignore
/// let leaving = hang_watch::enter(Station::WebRetire);
/// web.close(compositor);
/// hang_watch::at(leaving);
/// ```
///
/// Not a guard type: a guard would run on the unwind path too, and the one
/// thing this module must never do is add a `Drop` to a thread that is already
/// in trouble.
#[must_use]
pub fn enter(station: Station) -> Location {
    HEARTBEAT.enter_at(station, 0, HEARTBEAT.now_ms())
}

/// Run one existing call as an exclusive child station, then resume its parent.
///
/// The two station transitions are the two monotonic-clock reads this
/// instrumentation adds. Returning the parent's station after `work` rather
/// than using a drop guard also restores it on an ordinary `Result::Err`
/// without adding unwind work to a thread already in trouble.
pub fn during<T>(station: Station, work: impl FnOnce() -> T) -> T {
    let parent = enter(station);
    let output = work();
    at(parent);
    output
}

/// Same exclusive scope, keyed by the numeric tab/pane identity.
pub fn during_pane<T>(station: Station, pane: u64, work: impl FnOnce() -> T) -> T {
    let parent = enter_pane(station, pane);
    let output = work();
    at(parent);
    output
}

/// The window thread is handing control back. See [`Heartbeat::park`].
pub fn park(park: Park) {
    HEARTBEAT.park(park);
}

/// The platform woke the window thread. See [`Heartbeat::woke`].
pub fn woke() {
    HEARTBEAT.woke();
}

/// What the watchdog decided on one look.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// The pump is running; or it is parked with nothing owed; or it is quiet
    /// but not yet for long enough. Nothing to do and nothing to say.
    Quiet,
    /// It was quiet long enough to be asked, **and it answered**. Alive, and
    /// therefore not a fault — this is what a window being dragged by its edge
    /// for ten seconds looks like from here.
    ///
    /// Its own verdict rather than [`Self::Quiet`] because the two cost
    /// different things (this one asked a question) and because a test that
    /// could not tell them apart could not pin the difference between "never
    /// suspected" and "suspected and cleared".
    Excused { silent_ms: u64, station: Station },
    /// It was quiet, it was due back or holding control, and it did not answer.
    /// **Write a report.**
    Hung {
        silent_ms: u64,
        /// The threshold that was actually crossed, which is not always
        /// [`HANG_THRESHOLD`] — a loop that has not taken its first turn is
        /// judged against [`STARTUP_THRESHOLD`], and a report that named the
        /// wrong one would be a report that lies about its own trigger.
        threshold_ms: u64,
        /// How far past its own deadline a parked thread is, when that is what
        /// raised the suspicion. `None` when the thread was holding control, in
        /// which case there was no deadline to be past.
        overdue_ms: Option<u64>,
        /// What the window said when it was asked. Never [`Answer::Answered`] —
        /// that is [`Self::Excused`].
        answer: Answer,
        station: Station,
        turn: u64,
    },
    /// Still stopped, and already reported. Say nothing more.
    StillHung { silent_ms: u64 },
    /// The pump came back after a report was written. **Append one line.**
    ///
    /// Only ever after a [`Self::Hung`], which is what keeps the word honest:
    /// waking out of a park was never an illness, so it is never a recovery.
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
    /// When the window thread was last put a question (R2-28).
    ///
    /// It exists for one arm — an indefinite park, where the thread has said
    /// nothing is owed to it and the silence never ends on its own. A window
    /// nobody is using is in that state for hours, and asking it on every poll
    /// would be a message sent to an idle program every two seconds for as long
    /// as it is left alone. Once per threshold is the cadence instead: enough to
    /// find a thread that has stopped answering inside one more threshold, and
    /// far too little to be a cost.
    asked_at_ms: Option<u64>,
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
            asked_at_ms: None,
        }
    }

    /// One look. `now_ms` and `pulse` are read from the same clock.
    ///
    /// `ask` is the question of last resort — see
    /// [`bt_platform::hang::ask_thread_to_answer`] — and it is a parameter
    /// rather than something the caller does afterwards for two reasons. The
    /// whole decision stays in one function that a test can drive with numbers
    /// and a closure, which is what lets "a window that answers is never
    /// convicted" be a unit test instead of a person wedging a terminal. And it
    /// is **only invoked once suspicion is established**, so the ordinary
    /// two-second poll is still four atomic loads and a comparison: an idle
    /// Folio never sends a message to itself.
    pub fn poll(&mut self, now_ms: u64, pulse: Pulse, ask: &mut dyn FnMut() -> Answer) -> Verdict {
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
        // **Parking is not silence.** The window thread said, before it let go,
        // whether anything was owed to it; this is where that is spent.
        let overdue_ms = match pulse.park {
            // **Nothing was owed — and that is a reason not to *convict*, not a
            // reason not to ask** (R2-28). This arm used to answer `Quiet` here,
            // before the threshold and before the one question that can tell a
            // window nobody is using from a window that has stopped working; the
            // 200 reports that argument was written against were all filed on
            // arithmetic alone, and `ask` is what replaced the arithmetic.
            //
            // The state it was blind to is the one an indefinite park is most
            // likely to end in: a wake arrives — a shell speaks, a timer fires, a
            // message is posted — and the thread wedges on the way back out
            // before it writes a new pulse. Nothing about the pulse changes, so
            // every later poll reads the same indefinite park, and the report
            // that would name where it stopped is never written.
            //
            // A window that is genuinely idle answers the question and is
            // `Excused`, silently, which is what an idle Folio is. What that
            // costs is one message per threshold, and no more — see
            // [`Self::asked_at_ms`].
            Park::Indefinite => {
                if silent_ms < threshold_ms {
                    return Verdict::Quiet;
                }
                if self
                    .asked_at_ms
                    .is_some_and(|last| now_ms.saturating_sub(last) < threshold_ms)
                {
                    return Verdict::Quiet;
                }
                None
            }
            // Control was never handed over, so the loop coming round is owed by
            // this process to itself and the ordinary threshold applies.
            Park::Running => None,
            // A wake the platform promised. Suspicion is measured from the
            // deadline and not from the last turn, because the time before the
            // deadline was time the thread was *supposed* to be away — charging
            // it to the fault would report a five-second sleep as a five-second
            // hang.
            Park::Until(deadline) => {
                let overdue = now_ms.saturating_sub(deadline);
                if now_ms <= deadline || overdue < threshold_ms {
                    return Verdict::Quiet;
                }
                Some(overdue)
            }
        };
        if silent_ms < threshold_ms {
            return Verdict::Quiet;
        }
        // **Suspicion is not a verdict.** Everything above is arithmetic over
        // what this process said about itself; this is the one question put to
        // the outside, and a thread that answers it is alive whatever its own
        // loop is doing.
        self.asked_at_ms = Some(now_ms);
        let answer = ask();
        if answer == Answer::Answered {
            return Verdict::Excused {
                silent_ms,
                station: pulse.station,
            };
        }
        self.stall_began_ms = Some(pulse.at_ms);
        self.stall_station = pulse.station;
        Verdict::Hung {
            silent_ms,
            threshold_ms,
            overdue_ms,
            answer,
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
    /// How far past its own deadline a parked thread was. `None` when it was
    /// holding control.
    pub overdue_ms: Option<u64>,
    /// What the window said when it was asked, which is what turned suspicion
    /// into this file.
    pub answer: Answer,
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
        "The window thread was past due and did not answer when it was asked. It was\n\
         suspended for two kernel calls to take this sample and resumed immediately;\n\
         nothing here killed, restarted or unwedged anything. If there is no `healed` line\n\
         at the end of this file, the pump never came back before the process ended.\n\n",
    );
    // First of the facts, because every line under it — a module name, an
    // offset, a station — is only meaningful against the build it was taken
    // from. The same sentence `--version` prints (`crate::version`), so a report
    // and the person's own answer can be compared without translation.
    let _ = writeln!(out, "build          : {}", crate::version::banner());
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
        "parking        : {}",
        match facts.overdue_ms {
            Some(overdue) => format!(
                "it had parked with a deadline and is {} past it",
                seconds(overdue)
            ),
            None => "it was holding control, not parked".to_owned(),
        }
    );
    let _ = writeln!(out, "when asked     : {}", facts.answer.phrase());
    // **And which `WindowEvent` it was, which this same line answers.** A thread
    // wedged inside a handler is stamped at that handler's own station — see
    // [`Station::Event`] and the family under it — so `last station :
    // keyboard_input` names the kind as well as the call, and a separate
    // `last event` line would be one fact written twice and able to disagree
    // with itself.
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
        // The tally spells itself, so this footer and the decade lines
        // `bt-render` writes into `diagnostics.log` cannot drift apart.
        let _ = writeln!(out, "  surface acquires: {tally}");
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
/// `trace_perf` is whether this run was started with `BT_PERF_TRACE` set, and
/// it is the caller's answer for the same reason `reports` is: the variable is
/// read once per run, on the window thread, beside every other reader of it.
/// What it decides is the threshold — see [`TRACED_HANG_THRESHOLD`] — and it is
/// decided here rather than on the watchdog, which asks the environment
/// nothing.
///
/// Failing to spawn is said out loud and dropped: a terminal that refused to
/// start because it could not arrange to diagnose itself would be a worse
/// program than one that starts without the diagnosis.
pub fn start(reports: PathBuf, trace_perf: bool) {
    CPU_ARMED.store(true, Ordering::Relaxed);
    let ui_thread_id = bt_platform::hang::current_thread_id();
    // Touch the heartbeat here so its origin is the start of the run rather
    // than the first station, which makes `uptime` in a report mean what it
    // says.
    let _ = heartbeat().now_ms();
    let threshold = if trace_perf {
        TRACED_HANG_THRESHOLD
    } else {
        HANG_THRESHOLD
    };
    if let Err(error) = bt_platform::spawn_at_priority(
        "bt-hang-watch",
        bt_platform::ThreadPriority::BelowNormal,
        move || watch_forever(reports, ui_thread_id, threshold, trace_perf),
    ) {
        crate::diagnostics::note(&format!("Folio could not start its hang watchdog: {error}"));
    }
}

/// **Whether the watchdog currently believes the window thread has stopped.**
///
/// Written by the watchdog thread and read by anybody, which is the point: it is the one fact in
/// this module that another thread needs and cannot work out for itself. The launch endpoint reads
/// it before it tells a second `folio.exe` that its request has been taken — see
/// [`window_thread_can_serve`].
static WINDOW_THREAD_HUNG: AtomicBool = AtomicBool::new(false);

/// **Whether the window thread can be expected to come round and do something** (review C-2,
/// 2026-09-11).
///
/// Asked from **another thread**, which is why it is here rather than derived at the call site:
/// the two facts it rests on are the ones this module already stamps, and a second opinion about
/// liveness kept somewhere else would be a second answer to drift.
///
/// `allowance` is how long the asker is prepared to wait. Passing it in rather than fixing a number
/// is what keeps this honest: the launch endpoint's caller waits two seconds and then opens its own
/// window, so "can it come round" means "within two seconds" **for that caller** and would mean
/// something else for another.
#[must_use]
pub fn window_thread_can_serve(allowance: Duration) -> bool {
    let heart = heartbeat();
    !WINDOW_THREAD_HUNG.load(Ordering::Relaxed)
        && can_come_round(
            heart.now_ms(),
            heart.sample(),
            u64::try_from(allowance.as_millis()).unwrap_or(u64::MAX),
        )
}

/// The arithmetic half of [`window_thread_can_serve`], held where it can be put a table.
///
/// The three parks, and each answers a different question:
///
/// * **Indefinite** — nothing is owed and nothing is late. This is an idle window, and a nudge
///   wakes it; there is no length of silence here that says otherwise, which is [`Park`]'s own
///   founding note. What catches a thread that wedged *out of* an indefinite park is the flag the
///   watchdog sets, not this.
/// * **Running** — the loop holds control and owes itself a turn, so silence past the allowance is
///   a loop that will not come round inside it.
/// * **Until** — the platform owes a wake at a deadline, so the clock runs from the deadline and
///   not from the last turn. A thread that is asleep until tomorrow is not late today.
#[must_use]
pub fn can_come_round(now_ms: u64, pulse: Pulse, allowance_ms: u64) -> bool {
    match pulse.park {
        Park::Indefinite => true,
        Park::Running => now_ms.saturating_sub(pulse.at_ms) < allowance_ms,
        Park::Until(deadline) => now_ms.saturating_sub(deadline) < allowance_ms,
    }
}

/// The watchdog thread's whole life.
///
/// `threshold` is [`start`]'s answer and not this thread's: see
/// [`TRACED_HANG_THRESHOLD`] for which run gets which, and for why the reading
/// is not taken here.
fn watch_forever(reports: PathBuf, ui_thread_id: u32, threshold: Duration, trace_perf: bool) {
    let mut watch = HangWatch::new(threshold, STARTUP_THRESHOLD);
    let mut reads = crate::file_reads::Clock::default();
    // The file the stall in progress was reported to, so its healing line lands
    // in the same file rather than in a second one nobody would connect to it.
    let mut open_report: Option<PathBuf> = None;
    // The question, bound to the thread it is about. Not called unless the
    // arithmetic has already run out of innocent explanations.
    let mut ask = move || bt_platform::hang::ask_thread_to_answer(ui_thread_id, ANSWER_WITHIN);
    loop {
        std::thread::sleep(WATCH_INTERVAL);
        let heart = heartbeat();
        // **The other instrument, drained first and said last** (X-7): a hold
        // that ran long and then healed is exactly what the poll below is about
        // to call `Quiet`, so the holds are read before the verdict and land in
        // the log ahead of it, in the order the two facts happened. What moved
        // is the *writing*: nothing at all is written until the arithmetic has
        // run and, if it found a stall, the report file is on the disk. A
        // watchdog that printed first could be parked in that print — behind a
        // console nobody is reading — at the moment it was supposed to be
        // taking the stack of a window that had stopped.
        let (slow, dropped) = heart.take_slow_holds();
        // What the report attempt below left to say, kept until it has said
        // everything the file can hold.
        let mut reported: Option<String> = None;
        // Four atomic loads and a clock read. This is the entire steady-state
        // cost of the facility.
        let now_ms = heart.now_ms();
        match watch.poll(now_ms, heart.sample(), &mut ask) {
            // `Excused` says nothing out loud on purpose: a window that is being
            // dragged answers this every two seconds, and a diagnostic that
            // narrated it would be a log full of a program working.
            // **The flag other threads read, kept in step with the verdict and nowhere else**
            // (review C-2). `Quiet` and `Excused` are the arithmetic finding nothing wrong, which
            // is the only thing that clears it besides a healing; `StillHung` is the stall going
            // on, so it stays.
            Verdict::Quiet | Verdict::Excused { .. } => {
                WINDOW_THREAD_HUNG.store(false, Ordering::Relaxed);
            }
            Verdict::StillHung { .. } => {
                WINDOW_THREAD_HUNG.store(true, Ordering::Relaxed);
            }
            Verdict::Hung {
                silent_ms,
                threshold_ms,
                overdue_ms,
                answer,
                station,
                turn,
            } => {
                WINDOW_THREAD_HUNG.store(true, Ordering::Relaxed);
                let report = write_report(
                    &reports,
                    ui_thread_id,
                    Stall {
                        silent_ms,
                        threshold_ms,
                        overdue_ms,
                        answer,
                        station,
                        turn,
                    },
                    heart.now_ms(),
                );
                open_report = report.path;
                reported = Some(report.said);
            }
            Verdict::Healed { hung_ms, station } => {
                WINDOW_THREAD_HUNG.store(false, Ordering::Relaxed);
                if let Some(path) = open_report.take() {
                    append_healed(&path, hung_ms, station);
                }
            }
        }
        // **Everything this turn has to say, now that the disk has it.**
        // `diagnostics::note` and not `eprintln!`: a log file of this process's
        // own, reached by a handle of its own, so that a console somebody
        // stopped reading cannot hold the one thread that is still working.
        for hold in slow {
            crate::diagnostics::note(&hold.line());
        }
        if dropped > 0 {
            crate::diagnostics::note(&format!("Folio: {dropped} more slow turns went unrecorded"));
        }
        if let Some(said) = reported {
            crate::diagnostics::note(&said);
        }
        reads.tick(
            now_ms,
            trace_perf,
            &bt_platform::file_reads::LEDGER,
            bt_platform::file_reads::take_input,
            |line| crate::diagnostics::note(&line),
            crate::trace_sink::stderr_line,
        );
    }
}

/// Everything [`HangWatch::poll`] decided about one stall, carried in one piece.
///
/// A struct because the alternative was an eighth positional argument to
/// [`write_report`], and four consecutive integers whose order only the compiler
/// checks is how a report comes to print the threshold in the silence's place.
#[derive(Clone, Copy, Debug)]
struct Stall {
    silent_ms: u64,
    threshold_ms: u64,
    overdue_ms: Option<u64>,
    answer: Answer,
    station: Station,
    turn: u64,
}

/// **What one report attempt left behind**: the file, for the healing line that
/// belongs in it, and the one sentence the log is owed about it.
///
/// The sentence is **answered and not printed** (X-7). Writing it here would put
/// the watchdog's only output inside the function that takes a stopped thread's
/// stack, on a channel that may be a console nobody is reading — so the caller
/// says it, after this has returned and the evidence is already on the disk.
struct Reported {
    path: Option<PathBuf>,
    said: String,
}

/// Take the sample and put it on the disk. Answers where it landed and what to
/// say about it.
fn write_report(reports: &Path, ui_thread_id: u32, stall: Stall, uptime_ms: u64) -> Reported {
    let Stall {
        silent_ms,
        threshold_ms,
        overdue_ms,
        answer,
        station,
        turn,
    } = stall;
    // **The suspend happens here and nowhere else.** Everything above is
    // arithmetic; everything below is formatting.
    let attempt = HEARTBEAT.detail.active_attempt().unwrap_or_default();
    let stack = bt_platform::hang::capture_thread_stack(ui_thread_id, MAX_FRAMES);
    let timestamp = utc_timestamp(SystemTime::now());
    let facts = ReportFacts {
        written_at: &timestamp,
        process_id: std::process::id(),
        ui_thread_id,
        uptime_ms,
        silent_ms,
        threshold_ms,
        overdue_ms,
        answer,
        station,
        turn,
        stack: &stack,
        surfaces: bt_render::surface_failure_tally(),
    };
    let mut body = render_report(&facts);
    if !attempt.is_empty() {
        body.push_str(&format!("\npresent attempt:{attempt}\n"));
    }
    // Created lazily: a run that never hangs never makes this directory.
    if let Err(error) = fs::create_dir_all(reports) {
        return Reported {
            path: None,
            said: format!(
                "Folio saw its window thread stop for {} at {station}{attempt} but could not create {}: \
                 {error}",
                seconds(silent_ms),
                reports.display()
            ),
        };
    }
    let _ = prune_reports(reports, REPORTS_KEPT.saturating_sub(1));
    let path = reports.join(report_filename(&timestamp));
    match File::create(&path).and_then(|mut file| file.write_all(body.as_bytes())) {
        Ok(()) => Reported {
            said: format!(
                "Folio's window thread has not answered for {}; last station {station}{attempt}. Report: {}",
                seconds(silent_ms),
                path.display()
            ),
            path: Some(path),
        },
        Err(error) => Reported {
            said: format!("Folio could not write {}: {error}{attempt}", path.display()),
            path: None,
        },
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
///
/// The type is spelled in full rather than imported, because the import would
/// be unused in every build this item is compiled out of — which is every
/// release build, and therefore a warning on the profile that ships.
#[cfg(debug_assertions)]
static SELFTEST_FIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    #[test]
    fn native_cpu_sampler_answers_with_a_monotonic_thread_counter() {
        let first = bt_platform::mem::thread_cpu_us();
        let second = bt_platform::mem::thread_cpu_us();
        match (first, second) {
            (Some(first), Some(second)) => assert!(second >= first),
            (None, None) => {} // Platforms without a thread counter omit the field.
            _ => panic!("thread counter availability changed between adjacent reads"),
        }
    }
    #[test]
    fn event_kinds_have_distinct_value_labels_including_each_ime_variant() {
        use super::Station;
        use winit::event::{Ime, WindowEvent};
        for (event, expected) in [
            (WindowEvent::Ime(Ime::Enabled), Station::ImeEnabled),
            (
                WindowEvent::Ime(Ime::Preedit("中".into(), Some((3, 3)))),
                Station::ImePreedit,
            ),
            (
                WindowEvent::Ime(Ime::Commit("中".into())),
                Station::ImeCommit,
            ),
            (WindowEvent::Ime(Ime::Disabled), Station::ImeDisabled),
            (WindowEvent::CloseRequested, Station::EventClose),
            (WindowEvent::Destroyed, Station::EventDestroyed),
            (WindowEvent::RedrawRequested, Station::EventRedraw),
            (WindowEvent::Occluded(true), Station::EventOccluded),
            (WindowEvent::Occluded(false), Station::EventOccluded),
            (WindowEvent::Focused(true), Station::EventFocus),
            (WindowEvent::Focused(false), Station::EventFocus),
            (
                WindowEvent::ThemeChanged(winit::window::Theme::Dark),
                Station::EventTheme,
            ),
            (
                WindowEvent::DroppedFile("synthetic.txt".into()),
                Station::EventFileDrop,
            ),
            (
                WindowEvent::HoveredFile("synthetic.txt".into()),
                Station::EventHoveredFile,
            ),
            (
                WindowEvent::HoveredFileCancelled,
                Station::EventHoverCancelled,
            ),
            (
                WindowEvent::Resized(winit::dpi::PhysicalSize::new(1, 1)),
                Station::EventResize,
            ),
            (
                WindowEvent::Moved(winit::dpi::PhysicalPosition::new(0, 0)),
                Station::EventMoved,
            ),
        ] {
            assert_eq!(crate::window_event_station(&event), expected);
        }
    }

    #[test]
    fn nested_production_transitions_preserve_the_complete_exclusive_sum() {
        use super::{Heartbeat, Location, Park, Station};
        let heart = Heartbeat::sampling(|| None);
        heart.woke_at(0);
        heart.at_station(Station::Event, 10);
        let Location::Resume {
            station,
            node,
            scope,
        } = heart.enter_at(Station::ImeCommit, 0, 20)
        else {
            unreachable!()
        };
        let Location::Resume {
            station: caller,
            node: call_node,
            scope: call_scope,
        } = heart.enter_at(Station::PtyInput, 0, 30)
        else {
            unreachable!()
        };
        heart.resume_at(caller, call_node, call_scope, 1330);
        heart.resume_at(station, node, scope, 1340);
        heart.park_at(Park::Indefinite, 1350);
        let hold = heart.take_slow_holds().0.remove(0);
        assert_eq!(hold.spent_ms.iter().sum::<u64>(), hold.held_ms);
        assert_eq!(hold.detail.total_ms(), hold.held_ms);
        assert_eq!(hold.held_ms, 1350);
        assert!(hold.line().contains(
            "window_event 20 ms (IME Commit 20 ms (PtySession::write input enqueue 1300 ms))"
        ));
        assert_eq!(hold.line().lines().count(), 1);
    }
    #[test]
    fn thread_cpu_is_sampled_at_open_and_only_on_admitted_slow_close() {
        use std::cell::RefCell;
        thread_local! {
            static CPU: RefCell<(usize, std::collections::VecDeque<Option<u64>>)> =
                RefCell::new((0, [Some(100), Some(200), Some(3200), None, Some(4000), None].into()));
        }
        fn sample() -> Option<u64> {
            CPU.with(|state| {
                let mut state = state.borrow_mut();
                state.0 += 1;
                state.1.pop_front().unwrap()
            })
        }
        let mut heart = super::Heartbeat::sampling(|| None);
        heart.cpu_time = sample;
        heart.woke_at(0);
        heart.beat_at(1); // same hold, no second opening sample
        heart.park_at(super::Park::Indefinite, 10);
        assert_eq!(CPU.with(|state| state.borrow().0), 1);
        heart.woke_at(1000);
        heart.park_at(super::Park::Indefinite, 2300);
        let hold = heart.take_slow_holds().0.remove(0);
        assert_eq!(hold.cpu_us, Some(3000));
        assert!(hold.line().contains("thread CPU 3.000 ms / wall 1300 ms"));
        heart.woke_at(3000); // refused baseline: don't ask for an end
        heart.park_at(super::Park::Indefinite, 4300);
        assert_eq!(heart.take_slow_holds().0[0].cpu_us, None);
        assert_eq!(CPU.with(|state| state.borrow().0), 4);
        heart.woke_at(5000);
        heart.park_at(super::Park::Indefinite, 6300); // refused end
        assert_eq!(heart.take_slow_holds().0[0].cpu_us, None);
        assert_eq!(CPU.with(|state| state.borrow().0), 6);
    }

    /// A measurement, never a wall-clock acceptance gate; no GUI or process control.
    #[test]
    #[ignore = "explicit instrumentation cost measurement"]
    fn measure_cpu_sampler_and_station_cost() {
        use std::hint::black_box;
        use std::sync::atomic::Ordering;
        let iterations = 200_000_u32;
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            black_box(bt_platform::mem::thread_cpu_us());
        }
        eprintln!(
            "CPU sampler: {:.1} ns/read",
            start.elapsed().as_nanos() as f64 / f64::from(iterations)
        );
        let heart = super::Heartbeat::sampling(|| None);
        heart.woke_at(0);
        // Baseline transition copied from the pre-detail ledger: no tree work.
        // This is measurement only, not a second implementation used by tests.
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            let parent = black_box(heart.sample().station);
            for station in [super::Station::PtyInput, parent] {
                let now = heart.now_ms();
                let leaving = super::Station::from_byte(heart.station.load(Ordering::Relaxed));
                let since = heart.station_since_ms.load(Ordering::Relaxed);
                heart.spent_ms[leaving.slot()]
                    .fetch_add(now.saturating_sub(since), Ordering::Relaxed);
                heart.station_since_ms.store(now, Ordering::Relaxed);
                heart.station.store(station as u8, Ordering::Relaxed);
                heart.park.store(super::PARK_RUNNING, Ordering::Relaxed);
            }
        }
        eprintln!(
            "baseline enter/leave pair: {:.1} ns",
            start.elapsed().as_nanos() as f64 / f64::from(iterations)
        );
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            let super::Location::Resume {
                station,
                node,
                scope,
            } = heart.enter_at(super::Station::PtyInput, 0, heart.now_ms())
            else {
                unreachable!()
            };
            heart.resume_at(station, node, scope, heart.now_ms());
        }
        eprintln!(
            "station enter/leave pair: {:.1} ns",
            start.elapsed().as_nanos() as f64 / f64::from(iterations)
        );
    }
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{
        Answer, Footprint, HangWatch, Heartbeat, Location, Paging, Park, Pulse, ReportFacts,
        STATION_COUNT, SlowHold, Stall, Station, Verdict, can_come_round, prune_reports,
        render_healed, render_report, report_filename, slow_hold_threshold_ms, utc_timestamp,
        write_report,
    };

    /// A heartbeat on a platform that counts nothing, which is what every test
    /// about the **stations** wants: the line it prints is the one this module
    /// wrote before the counters existed, and the arithmetic under test is the
    /// milliseconds.
    fn no_footprint() -> Option<Footprint> {
        None
    }

    // The footprints `fake_footprint` hands back, in order, and how many times
    // it has been asked. A `//` comment and not a `///` one: the item is inside
    // a macro, so a doc comment here documents nothing and `unused_doc_comment`
    // says so.
    //
    // Thread-local because libtest gives each case its own thread and runs them
    // at once: a static here would have two tests handing each other their
    // numbers, which is the kind of shared fixture this repository's own
    // conventions call a bug factory.
    thread_local! {
        static FAKE_FOOTPRINTS: RefCell<(Vec<Footprint>, usize)> =
            const { RefCell::new((Vec::new(), 0)) };
    }

    /// A sampler that answers the queued footprints in turn, and nothing once
    /// they run out.
    fn fake_footprint() -> Option<Footprint> {
        FAKE_FOOTPRINTS.with(|fake| {
            let mut fake = fake.borrow_mut();
            let asked = fake.1;
            fake.1 += 1;
            fake.0.get(asked).copied()
        })
    }

    /// Queue what the fake sampler will answer, and forget what it was asked
    /// before.
    fn queue_footprints(footprints: &[(u64, u64)]) {
        let queued: Vec<Footprint> = footprints
            .iter()
            .map(|(faults, working_set_bytes)| Footprint {
                faults: *faults,
                working_set_bytes: *working_set_bytes,
            })
            .collect();
        FAKE_FOOTPRINTS.with(|fake| *fake.borrow_mut() = (queued, 0));
    }

    /// How many times the fake sampler has been asked since [`queue_footprints`].
    fn footprints_asked() -> usize {
        FAKE_FOOTPRINTS.with(|fake| fake.borrow().1)
    }

    /// A thread holding control at `station`: it has not handed anything back to
    /// the platform, so the loop coming round is owed.
    fn pulse(at_ms: u64, turn: u64, station: Station) -> Pulse {
        Pulse {
            at_ms,
            turn,
            station,
            park: Park::Running,
        }
    }

    /// A thread parked with no deadline — an idle window, which is the shape 200
    /// of the first 205 reports were.
    fn idle(at_ms: u64, turn: u64) -> Pulse {
        Pulse {
            at_ms,
            turn,
            station: Station::Parked,
            park: Park::Indefinite,
        }
    }

    /// **RED (review C-2, 2026-09-11) — whether the loop can be expected to come round inside
    /// somebody else's allowance, asked from another thread.**
    ///
    /// The launch endpoint's question, and the reason it is asked at all: before this, the listener
    /// thread told a second `folio.exe` its request had been taken on nothing but the grammar of
    /// the line — so a Folio whose window thread had stopped answered `yes` in microseconds and the
    /// person who started Folio again got exit code 0 and no window.
    ///
    /// MUTATIONS: answer `false` for an indefinite park and every idle Folio starts refusing
    /// launches, which is the fault this whole enum exists to prevent ([`Park::Indefinite`]);
    /// measure a deadlined park from its last turn instead of from its deadline, and a window
    /// asleep until tomorrow is called dead today.
    #[test]
    fn a_loop_that_cannot_come_round_inside_the_allowance_says_so() {
        const ALLOWANCE: u64 = 2_000;
        assert!(
            can_come_round(9_000_000, idle(1, 1), ALLOWANCE),
            "an idle window is owed nothing and is woken by the nudge, however long it has been \
             quiet"
        );
        assert!(can_come_round(
            2_500,
            pulse(1_000, 1, Station::Drain),
            ALLOWANCE
        ));
        assert!(
            !can_come_round(3_100, pulse(1_000, 1, Station::Drain), ALLOWANCE),
            "a loop holding control and silent past the allowance will not come round inside it"
        );
        assert!(
            can_come_round(60_000, parked_until(1_000, 1, 59_000), ALLOWANCE),
            "a wake is owed a second from now, which is inside the allowance"
        );
        assert!(
            !can_come_round(60_000, parked_until(1_000, 1, 50_000), ALLOWANCE),
            "the platform owed a wake ten seconds ago and it has not arrived"
        );
    }

    /// A thread parked until `deadline`, on the heartbeat's own clock.
    fn parked_until(at_ms: u64, turn: u64, deadline: u64) -> Pulse {
        Pulse {
            at_ms,
            turn,
            station: Station::Parked,
            park: Park::Until(deadline),
        }
    }

    /// A question and a tally of how often it was put. `answer` is what the
    /// window says every time it is asked.
    struct Question {
        answer: Answer,
        asked: std::cell::Cell<usize>,
    }

    impl Question {
        fn answering(answer: Answer) -> Self {
            Self {
                answer,
                asked: std::cell::Cell::new(0),
            }
        }

        /// Nothing on the other end — the shape of every real hang.
        fn silent() -> Self {
            Self::answering(Answer::Silent)
        }

        fn ask(&self) -> impl FnMut() -> Answer + '_ {
            move || {
                self.asked.set(self.asked.get() + 1);
                self.answer
            }
        }

        fn asked(&self) -> usize {
            self.asked.get()
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
        let question = Question::silent();
        let mut ask = question.ask();
        // Three healthy turns.
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(3_000, pulse(2_900, 2, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(5_000, pulse(4_900, 3, Station::Drain), &mut ask),
            Verdict::Quiet
        );
        // The pump stops at 4_900 with `Drain` as its last station, holding
        // control — it never reached the hand-back, so nothing is parked.
        assert_eq!(
            watch.poll(7_000, pulse(4_900, 3, Station::Drain), &mut ask),
            Verdict::Quiet,
            "2.1s of silence is not yet a hang"
        );
        assert_eq!(
            watch.poll(9_900, pulse(4_900, 3, Station::Drain), &mut ask),
            Verdict::Hung {
                silent_ms: 5_000,
                threshold_ms: 5_000,
                overdue_ms: None,
                answer: Answer::Silent,
                station: Station::Drain,
                turn: 3,
            },
            "the threshold is crossed and the one report is owed here"
        );
        assert_eq!(
            watch.poll(11_900, pulse(4_900, 3, Station::Drain), &mut ask),
            Verdict::StillHung { silent_ms: 7_000 },
            "and never a second report for the same stall"
        );
        assert_eq!(
            watch.poll(13_900, pulse(4_900, 3, Station::Drain), &mut ask),
            Verdict::StillHung { silent_ms: 9_000 }
        );
        // The pump comes back: its next turn stamps 13_400.
        assert_eq!(
            watch.poll(15_900, pulse(13_400, 4, Station::Wait), &mut ask),
            Verdict::Healed {
                hung_ms: 8_500,
                station: Station::Drain,
            },
            "13_400 - 4_900, both written by the window thread, and not 15_900 - 4_900"
        );
        assert_eq!(
            watch.poll(17_900, pulse(15_900, 5, Station::Wait), &mut ask),
            Verdict::Quiet,
            "a recovered watch is armed again and silent again"
        );
        drop(ask);
        assert_eq!(
            question.asked(),
            1,
            "the window is asked once — at the moment suspicion arose — and not \
             on the healthy polls, nor again while the stall it already reported \
             is still standing"
        );
    }

    /// PIN — **a second stall after a recovery is reported again.** The state
    /// that stops the second report is the state a recovery must clear; a watch
    /// that only latched would witness one hang per process lifetime, which for
    /// an intermittent fault is the wrong one.
    #[test]
    fn a_second_stall_after_a_recovery_earns_its_own_report() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        let question = Question::silent();
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        assert!(matches!(
            watch.poll(7_000, pulse(900, 1, Station::Present), &mut ask),
            Verdict::Hung { .. }
        ));
        assert!(matches!(
            watch.poll(9_000, pulse(8_000, 2, Station::Wait), &mut ask),
            Verdict::Healed { .. }
        ));
        assert!(matches!(
            watch.poll(15_000, pulse(8_000, 2, Station::WebPage), &mut ask),
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
        let question = Question::silent();
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(1_000, pulse(1_000, 1, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        // Same turn, ancient stamp, control never handed back: a hang.
        assert!(matches!(
            watch.poll(9_000, pulse(1_000, 1, Station::Wait), &mut ask),
            Verdict::Hung { .. }
        ));
        // A different watch: the turn moves but the stamp does not advance —
        // two turns inside one millisecond, which is what a busy loop looks
        // like. That is a live pump and owes no report.
        let mut busy = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(
            busy.poll(1_000, pulse(1_000, 1, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        assert_eq!(
            busy.poll(9_000, pulse(1_000, 2, Station::Wait), &mut ask),
            Verdict::Quiet
        );
    }

    /// PIN (misreport, 2026-08-25; 200 of the first 205 reports) — **an idle
    /// window is not a hang, however long it is idle.**
    ///
    /// The evidence: a user running Claude Code inside a Folio pane watched
    /// `Folio's window thread has not answered for 5.748s; last station
    /// flush_pending_pty_resize` appear in the input box, over and over, about
    /// once every eight seconds. Symbolised, 200 of the 205 reports had the same
    /// `rip` — `win32u!NtUserMsgWaitForMultipleObjectsEx+0x14` — which is the
    /// window thread **legitimately parked** in `ControlFlow::Wait`, and the
    /// station they all blamed had returned successfully on the quiet turn
    /// before the silence started.
    ///
    /// Red gate: judge on the turn counter alone — the whole of the first
    /// version — and the assertions below turn into `Hung` in an event-driven
    /// GUI that is working exactly as designed.
    ///
    /// **What R2-28 moved is *why* it is never a hang**, not whether. The park
    /// reason used to answer the question on its own, before the threshold and
    /// before anything was asked; now it decides what the arithmetic means and
    /// the window is asked past the threshold like any other. An idle window
    /// answers — it is sitting in `MsgWaitForMultipleObjectsEx`, which is where a
    /// sent message is delivered from — so it is `Excused`, which says nothing
    /// out loud and writes no file. The question is put once per threshold and
    /// not once per poll, which is the whole of what the old arm was buying.
    #[test]
    fn a_window_parked_with_nothing_owed_is_never_a_hang() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        let question = Question::answering(Answer::Answered);
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        // The turn ends, the loop asks for `ControlFlow::Wait`, and nobody
        // types for a quarter of an hour.
        assert_eq!(
            watch.poll(3_000, idle(900, 1), &mut ask),
            Verdict::Quiet,
            "2.1 seconds of silence is under the threshold, so nothing is asked"
        );
        for now in [7_000, 60_000, 900_000] {
            assert_eq!(
                watch.poll(now, idle(900, 1), &mut ask),
                Verdict::Excused {
                    silent_ms: now - 900,
                    station: Station::Parked,
                },
                "{now}ms: an idle window answers, and a window that answers is never convicted"
            );
        }
        drop(ask);
        assert_eq!(
            question.asked(),
            3,
            "once per threshold and not once per poll: three questions across \
             fifteen minutes of an idle window"
        );
    }

    /// RED (R2-28) — **a thread that wedges on the way out of an indefinite park
    /// is reported.**
    ///
    /// The state the old arm could not see, and the one an indefinite park is
    /// most likely to end in badly. The thread parks with nothing owed, which is
    /// correct and ordinary; a wake arrives — a shell speaks, a message is
    /// posted — and it wedges before it writes a new pulse. Nothing about the
    /// pulse changes, so the park reason left behind on the way *in* is what
    /// every later poll reads, and the arm answered `Quiet` to all of them. No
    /// file, no station, no evidence at all for the one fault this facility
    /// exists to report.
    ///
    /// MUTATION: put `Park::Indefinite => return Verdict::Quiet` back and every
    /// assertion below reads `Quiet`, for ever.
    #[test]
    fn a_wedge_on_the_way_out_of_an_idle_park_is_still_reported() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        let question = Question::silent();
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(3_000, idle(900, 1), &mut ask),
            Verdict::Quiet,
            "under the threshold nothing is suspected and nothing is asked"
        );
        assert_eq!(
            watch.poll(7_000, idle(900, 1), &mut ask),
            Verdict::Hung {
                silent_ms: 6_100,
                threshold_ms: 5_000,
                // Nothing was owed, so there is no deadline to be late for and
                // no overdue figure to print. The silence is the whole account.
                overdue_ms: None,
                answer: Answer::Silent,
                station: Station::Parked,
                turn: 1,
            },
            "a park that stopped answering is a wedge whatever it said on the way in"
        );
        assert_eq!(
            watch.poll(9_000, idle(900, 1), &mut ask),
            Verdict::StillHung { silent_ms: 8_100 },
            "and it is reported once"
        );
        // The wake path finishes, eventually.
        assert_eq!(
            watch.poll(11_000, idle(10_900, 2), &mut ask),
            Verdict::Healed {
                hung_ms: 10_000,
                station: Station::Parked,
            }
        );
    }

    /// PIN — **a wake the platform promised and did not deliver is a hang**, and
    /// it is measured from the deadline rather than from the last turn.
    ///
    /// The other half of the park: `ControlFlow::WaitUntil` is a claim on the
    /// platform, so silence past it is somebody's fault. Charging the whole
    /// silence to the fault would be the mirror error of the one above — a
    /// window that asked to sleep for a minute and did would be reported for
    /// fifty-five seconds of hang.
    #[test]
    fn a_deadline_that_passes_without_a_wake_is_a_hang_measured_from_the_deadline() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        let question = Question::silent();
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        // Parked at 900 with a deadline of 60_900 — a minute-long clock, which
        // is an ordinary thing for a quiet window to ask for.
        assert_eq!(
            watch.poll(30_000, parked_until(900, 1, 60_900), &mut ask),
            Verdict::Quiet,
            "29 seconds of silence, none of it owed yet"
        );
        assert_eq!(
            watch.poll(62_000, parked_until(900, 1, 60_900), &mut ask),
            Verdict::Quiet,
            "a second late is a scheduler, not a hang"
        );
        assert_eq!(
            watch.poll(65_900, parked_until(900, 1, 60_900), &mut ask),
            Verdict::Hung {
                silent_ms: 65_000,
                threshold_ms: 5_000,
                overdue_ms: Some(5_000),
                answer: Answer::Silent,
                station: Station::Parked,
                turn: 1,
            },
            "five seconds past a deadline it set itself, and it does not answer"
        );
        drop(ask);
        assert_eq!(question.asked(), 1);
    }

    /// PIN — **a window that answers is never convicted.**
    ///
    /// The case this is written for is a USER32 modal loop: a hand holding the
    /// window's edge, or a tracked menu. winit's own loop is genuinely not
    /// turning — the modal pump is inside `DefWindowProc` — and the application
    /// is genuinely fine, because it is pumping, repainting and answering. The
    /// same shape covers any turn that legitimately runs long.
    ///
    /// Red gate: convict on the arithmetic alone and every drag longer than five
    /// seconds writes a report, suspends the window thread to do it, and tells
    /// the user their terminal stopped answering while they were using it.
    #[test]
    fn a_thread_that_answers_is_alive_even_when_its_loop_has_stopped_turning() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        let question = Question::answering(Answer::Answered);
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        for now in [7_000, 9_000, 11_000] {
            assert_eq!(
                watch.poll(now, pulse(900, 1, Station::Event), &mut ask),
                Verdict::Excused {
                    silent_ms: now - 900,
                    station: Station::Event,
                },
                "{now}ms: the loop is not turning and the window is answering"
            );
        }
        drop(ask);
        assert_eq!(
            question.asked(),
            3,
            "asked on every suspicious poll, because the answer can change — and \
             it is the answer, not a latch, that excuses it"
        );

        // And the same watch, once the answering stops, is not disarmed by
        // having excused: an excuse is not a verdict either.
        let gone = Question::silent();
        let mut ask = gone.ask();
        assert!(matches!(
            watch.poll(13_000, pulse(900, 1, Station::Event), &mut ask),
            Verdict::Hung {
                station: Station::Event,
                answer: Answer::Silent,
                ..
            }
        ));
    }

    /// PIN — **a start with no window yet is still reported.** There is nothing
    /// to ask before the event loop exists, and "the question could not be put"
    /// must not read as "it answered" — a window that never appears is the
    /// loudest hang there is, and it is the one with no `HWND` to ask.
    #[test]
    fn a_start_with_no_window_to_ask_is_convicted_on_its_own_grace() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        let question = Question::answering(Answer::NoWindow);
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(8_000, pulse(0, 0, Station::Starting), &mut ask),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(31_000, pulse(0, 0, Station::Starting), &mut ask),
            Verdict::Hung {
                silent_ms: 31_000,
                threshold_ms: 30_000,
                overdue_ms: None,
                answer: Answer::NoWindow,
                station: Station::Starting,
                turn: 0,
            }
        );
    }

    /// PIN — **a thread wedged at a named station is still caught.** The measured
    /// one — 131 seconds, `IsHungAppWindow` true, the UI thread burning a core —
    /// and the deliberate one `BT_HANG_SELFTEST` performs are the same shape:
    /// the thread never reached the hand-back, so it is not parked; its station
    /// is the call it is stuck in and **not** `Parked`, because `at` clears the
    /// parking on the way in; and it does not answer.
    ///
    /// This is the direction the parking fix could have broken, and the reason
    /// the station has to be the real one: a report that said `parked` for a
    /// thread asleep inside `BT_HANG_SELFTEST` would have thrown away the only
    /// piece of evidence the facility exists to produce.
    #[test]
    fn a_thread_wedged_at_a_station_is_reported_at_that_station() {
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        let question = Question::silent();
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(3_000, pulse(2_900, 4, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        // The selftest holds the thread from 2_900 onwards, at its own station.
        assert_eq!(
            watch.poll(9_000, pulse(2_900, 4, Station::SelfTest), &mut ask),
            Verdict::Hung {
                silent_ms: 6_100,
                threshold_ms: 5_000,
                overdue_ms: None,
                answer: Answer::Silent,
                station: Station::SelfTest,
                turn: 4,
            }
        );
        // And a wedge in the pane drain, which is the fault this facility was
        // built for, reads the same way.
        let mut second = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(
            second.poll(3_000, pulse(2_900, 4, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        assert!(matches!(
            second.poll(134_000, pulse(2_900, 4, Station::Drain), &mut ask),
            Verdict::Hung {
                station: Station::Drain,
                answer: Answer::Silent,
                overdue_ms: None,
                ..
            }
        ));
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
        let question = Question::silent();
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(2_000, pulse(0, 0, Station::Starting), &mut ask),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(8_000, pulse(0, 0, Station::Starting), &mut ask),
            Verdict::Quiet,
            "eight seconds to build an event loop, a GPU device and a shell is a \
             slow start, not a hang — this is the false alarm the first real run filed"
        );
        assert_eq!(
            watch.poll(31_000, pulse(0, 0, Station::Starting), &mut ask),
            Verdict::Hung {
                silent_ms: 31_000,
                threshold_ms: 30_000,
                overdue_ms: None,
                answer: Answer::Silent,
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
        let question = Question::silent();
        let mut ask = question.ask();
        assert_eq!(
            watch.poll(1_000, pulse(900, 1, Station::Wait), &mut ask),
            Verdict::Quiet
        );
        assert_eq!(
            watch.poll(7_000, pulse(900, 1, Station::Drain), &mut ask),
            Verdict::Hung {
                silent_ms: 6_100,
                threshold_ms: 5_000,
                overdue_ms: None,
                answer: Answer::Silent,
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
            Station::Parked,
            Station::Woken,
        ] {
            heart.at(station);
            assert_eq!(
                heart.sample().station,
                station,
                "every station survives the byte it is stored as"
            );
        }
    }

    /// PIN — **the parking is recorded before control is given up and cleared by
    /// everything that takes it back.**
    ///
    /// The invariant the whole fix rests on: arriving anywhere named means
    /// holding control. If `at` did not clear the park, a wake that reached
    /// `window_event` and wedged there would still be wearing the previous
    /// turn's deadline — and would be excused by a promise about something else.
    #[test]
    fn parking_is_stamped_on_the_way_out_and_cleared_by_everything_on_the_way_in() {
        let heart = Heartbeat::new();
        assert_eq!(
            heart.sample().park,
            Park::Running,
            "a heartbeat is born holding control: nothing has parked yet"
        );

        heart.park(Park::Indefinite);
        assert_eq!(heart.sample().park, Park::Indefinite);
        assert_eq!(
            heart.sample().station,
            Station::Parked,
            "and it says so as a station, so a report can never blame the last \
             long call of the quiet turn before it"
        );

        heart.woke();
        assert_eq!(
            heart.sample().park,
            Park::Running,
            "a wake ends the park even though no turn has happened yet"
        );
        assert_eq!(heart.sample().station, Station::Woken);

        heart.park(Park::Until(9_000));
        assert_eq!(heart.sample().park, Park::Until(9_000));
        heart.at(Station::Event);
        assert_eq!(
            heart.sample().park,
            Park::Running,
            "entering a named call is holding control"
        );

        heart.park(Park::Until(9_000));
        heart.beat();
        assert_eq!(heart.sample().park, Park::Running, "and so is coming round");
        assert_eq!(heart.sample().station, Station::Wait);
    }

    /// PIN — **the parking survives the `u64` it is stored as**, including the
    /// two values that are not deadlines.
    ///
    /// The encoding hazard is real in one direction only: a deadline that
    /// collided with the "indefinite" pattern would make a late wake-up
    /// unreportable forever. `u64::MAX` milliseconds is 584 million years of
    /// uptime, so the collision is unreachable — this pins that the mapping is
    /// nonetheless total and round-trips.
    #[test]
    fn every_parking_survives_the_number_it_is_stored_as() {
        let heart = Heartbeat::new();
        for park in [
            Park::Running,
            Park::Indefinite,
            Park::Until(1),
            Park::Until(9_000),
            Park::Until(u64::MAX - 1),
        ] {
            heart.park(park);
            assert_eq!(heart.sample().park, park);
        }
        // A deadline of zero is unreachable — the origin is fixed before the
        // event loop exists — and is read as the earliest possible deadline
        // rather than as "not parked", because the alternative is a park that
        // silently becomes a hold on control.
        heart.park(Park::Until(0));
        assert_eq!(heart.sample().park, Park::Until(1));
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
            overdue_ms: None,
            answer: Answer::Silent,
            station: Station::SelfTest,
            turn: 4172,
            stack: &stack,
            surfaces: bt_render::SurfaceFailureTally {
                outdated: 3,
                ..bt_render::SurfaceFailureTally::default()
            },
        });
        assert!(
            report.contains(&crate::version::banner()),
            "a report is attributable to the build that wrote it, {report}"
        );
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
        assert!(
            report.contains("parking        : it was holding control, not parked"),
            "a report says which of the two suspicions raised it, {report}"
        );
        assert!(
            report.contains("when asked     : the window was asked and did not answer"),
            "and that the question was actually put — the line that separates \
             this file from the two hundred that were written without asking, \
             {report}"
        );
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
            overdue_ms: Some(7_250),
            answer: Answer::NoWindow,
            station: Station::Drain,
            turn: 0,
            stack: &stack,
            surfaces: bt_render::SurfaceFailureTally::default(),
        });
        assert!(
            clean.contains("surface acquires: clean"),
            "and a run with none says so in one word, {clean}"
        );
        assert!(
            clean.contains("parking        : it had parked with a deadline and is 7.250s past it"),
            "a wake the platform promised and did not deliver says how late it is, {clean}"
        );
        assert!(
            clean.contains("when asked     : this thread owned no window to ask"),
            "and a question that could not be put says that rather than passing \
             for an answer, {clean}"
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
            overdue_ms: None,
            answer: Answer::Silent,
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

    /// PIN — **the watchdog writes its report, and says so, while the trace
    /// sink is stalled and the process's `stderr` lock is held** (X-7).
    ///
    /// The two ways the console reaches back into this thread, both arranged at
    /// once: a sink whose writer is inside a write that will not return, and the
    /// lock every `eprintln!` in the workspace goes through, held by somebody
    /// else. The one thread whose entire job is to still be working when the
    /// window thread is not must come through both without waiting — so the
    /// report file appears, and the line about it reaches `diagnostics.log` by
    /// the road that owns no lock.
    ///
    /// The work runs on a second thread only so that this one can put a deadline
    /// on it: a regression here does not fail an assertion, it stops.
    ///
    /// MUTATION: put the `eprintln!` back in `write_report`; the report is never
    /// written and the deadline expires.
    #[test]
    fn a_report_and_its_line_do_not_wait_for_the_console() {
        let private = std::env::temp_dir().join(format!(
            "folio-hang-stalled-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&private);
        std::fs::create_dir_all(&private).expect("a private directory for this test");
        let reports = private.join("hang-reports");
        let log = private.join(crate::diagnostics::LOG_FILENAME);

        let sink = crate::trace_sink::StalledWriter::start();
        sink.fill();
        assert!(
            !sink.offer("one more"),
            "the sink under test is supposed to be full"
        );
        let (release, released) = std::sync::mpsc::sync_channel::<()>(0);
        let (locked, holding) = std::sync::mpsc::sync_channel::<()>(1);
        let holder = std::thread::spawn(move || {
            let _guard = std::io::stderr().lock();
            let _ = locked.send(());
            let _ = released.recv();
        });
        holding
            .recv_timeout(Duration::from_secs(5))
            .expect("the holder took the process lock");

        let (done, finished) = std::sync::mpsc::sync_channel::<bool>(1);
        let worker = {
            let reports = reports.clone();
            let log = log.clone();
            std::thread::spawn(move || {
                let reported = write_report(
                    &reports,
                    // The watchdog's own id: `capture_thread_stack` refuses to
                    // sample the thread that asked, so this exercises the
                    // report's every other step without suspending anything.
                    bt_platform::hang::current_thread_id(),
                    Stall {
                        silent_ms: 2_400,
                        threshold_ms: 2_000,
                        overdue_ms: None,
                        answer: Answer::Silent,
                        station: Station::Present,
                        turn: 91,
                    },
                    12_000,
                );
                let noted = crate::diagnostics::append_note(&log, &reported.said);
                let _ = done.send(reported.path.is_some() && noted);
            })
        };
        let completed = finished.recv_timeout(Duration::from_secs(10));
        drop(release);
        holder.join().unwrap();
        worker.join().unwrap();
        drop(sink);

        assert_eq!(
            completed,
            Ok(true),
            "the report path waited for a console it must not touch"
        );
        let written: Vec<PathBuf> = std::fs::read_dir(&reports)
            .expect("the reports directory was created")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        assert_eq!(written.len(), 1, "one stall, one report: {written:?}");
        let said = std::fs::read_to_string(&log).expect("the log took the line");
        assert!(
            said.contains("has not answered for 2.400s"),
            "the log says what happened and names the report, {said}"
        );
        assert!(said.ends_with('\n'), "one whole line, {said}");
        let _ = std::fs::remove_dir_all(&private);
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

    // ══ The ledger: a hold that ran long, and where its milliseconds went ══
    //
    // Every one of these drives the `_at` verbs, so the arithmetic is stated in
    // numbers rather than reproduced by sleeping — which is the whole reason
    // those verbs take a clock.

    /// **An ordinary hold says nothing.** Sixty of these a second, and the
    /// instrument has to be silent through all of them or it is not an
    /// instrument, it is the log.
    #[test]
    fn an_ordinary_hold_is_never_written_down() {
        let heart = Heartbeat::sampling(no_footprint);
        heart.woke_at(1_000);
        heart.at_station(Station::Event, 1_001);
        heart.beat_at(1_002);
        heart.at_station(Station::Present, 1_004);
        heart.park_at(Park::Indefinite, 1_012);
        assert_eq!(heart.take_slow_holds(), (Vec::new(), 0));
    }

    /// **A slow hold names the station that spent the time**, and names it
    /// first.
    ///
    /// Red gate: this is the report of 2026-08-30 — a window dead for a couple
    /// of seconds with a hosted page on it, healing on its own, and nothing in
    /// `diagnostics.log`, because the wedge watchdog above neither fires under
    /// five seconds nor says anything about a thread that answered.
    #[test]
    fn a_slow_hold_names_the_station_that_spent_the_time() {
        let heart = Heartbeat::sampling(no_footprint);
        heart.woke_at(1_000);
        heart.at_station(Station::Event, 1_010);
        heart.at_station(Station::WebPage, 1_020);
        heart.park_at(Park::Indefinite, 2_900);
        let (slow, dropped) = heart.take_slow_holds();
        assert_eq!(dropped, 0);
        let [hold] = slow.as_slice() else {
            panic!("one hold ran long, so one hold was recorded: {slow:?}");
        };
        assert_eq!(hold.held_ms, 1_900, "measured end to end, not summed");
        assert_eq!(hold.spent_ms[Station::WebPage.slot()], 1_880);
        assert_eq!(hold.spent_ms[Station::Event.slot()], 10);
        assert_eq!(hold.spent_ms[Station::Woken.slot()], 10);
        assert_eq!(hold.session_age_ms, 2_900);
        assert_eq!(hold.stall_count, 1);
        assert_eq!(
            hold.line(),
            "Folio: the window thread held control for 1900 ms on turn 0 — \
             advance_web_page 1880 ms, window_event 10 ms, woken 10 ms · \
             session age 2.900s, stall #1",
        );
    }

    /// **A child station owns its interval exclusively.** Returning to the
    /// parent starts a new parent interval; it does not make the child time a
    /// second copy inside the parent.
    #[test]
    fn redraw_substations_add_up_without_double_counting_the_parent() {
        let heart = Heartbeat::sampling(no_footprint);
        heart.woke_at(1_000);
        heart.at_station(Station::RenderCompose, 1_010);
        heart.at_station(Station::SurfaceAcquire, 1_110);
        heart.at_station(Station::RenderCompose, 1_610);
        heart.at_station(Station::QueueSubmit, 1_710);
        heart.at_station(Station::SwapchainPresent, 1_810);
        heart.at_station(Station::RenderCompose, 2_310);
        heart.park_at(Park::Indefinite, 2_410);
        let (holds, dropped) = heart.take_slow_holds();
        assert_eq!(dropped, 0);
        let [hold] = holds.as_slice() else {
            panic!("one synthetic redraw hold was recorded: {holds:?}")
        };
        assert_eq!(hold.spent_ms[Station::RenderCompose.slot()], 300);
        assert_eq!(hold.spent_ms[Station::SurfaceAcquire.slot()], 500);
        assert_eq!(hold.spent_ms[Station::QueueSubmit.slot()], 100);
        assert_eq!(hold.spent_ms[Station::SwapchainPresent.slot()], 500);
        assert_eq!(
            hold.spent_ms.iter().sum::<u64>(),
            hold.held_ms,
            "exclusive child intervals and their parent account for the hold once",
        );
    }

    /// **A wake carrying a worker's answer names the handler, not `woken`**
    /// (2026-09-11).
    ///
    /// Red gate: this is the report of 2026-09-10, whose whole account of a
    /// ninety-second stall was `woken 85613 ms` — a line that named the loop
    /// having been woken and nothing about what it then did. `user_event` now
    /// opens the station its own event chooses ([`crate::AppEvent::station`]),
    /// so the same stall says `apply_preview_results`, and the reader knows
    /// which lane to look down before reading a single line of code.
    ///
    /// MUTATION: drop the `enter`/`at` pair from `user_event` and the preview's
    /// eighty seconds go back to `woken`, which is this assertion inverted.
    #[test]
    fn a_hold_spent_on_a_workers_answer_names_the_lane_that_landed() {
        let heart = Heartbeat::sampling(no_footprint);
        heart.woke_at(1_000);
        // Ten milliseconds of untagged wake, then a preview body landing, then
        // the turn that draws it.
        heart.at_station(Station::Preview, 1_010);
        heart.at_station(Station::Woken, 2_010);
        heart.beat_at(2_020);
        heart.park_at(Park::Indefinite, 2_030);
        let (holds, dropped) = heart.take_slow_holds();
        assert_eq!(dropped, 0);
        let [hold] = holds.as_slice() else {
            panic!("one hold, and it ran long")
        };
        assert_eq!(hold.spent_ms[Station::Preview.slot()], 1_000);
        assert_eq!(hold.spent_ms[Station::Woken.slot()], 20);
        assert_eq!(
            hold.line(),
            "Folio: the window thread held control for 1030 ms on turn 1 — \
             apply_preview_results 1000 ms, woken 20 ms, about_to_wait 10 ms · \
             session age 2.030s, stall #1",
            "the handler is named, and named first",
        );
    }

    /// **Every lane a worker answers on has a station of its own**, so that no
    /// arm of `user_event` can go back to being charged to nobody.
    ///
    /// The labels are the function names a reader greps for, which is the whole
    /// value of the line: a station called `preview` would send them looking for
    /// a module, and `apply_preview_results` sends them to the door.
    #[test]
    fn every_worker_lane_is_labelled_with_the_function_it_names() {
        for (station, label) in [
            (Station::Preview, "apply_preview_results"),
            (Station::Math, "apply_math_results"),
            (Station::Files, "apply_files_results"),
            (Station::Git, "apply_git_results"),
            (Station::Attention, "raise_attention"),
            (Station::Picture, "adopt_background_picture"),
            (Station::FileIndex, "apply_file_index_results"),
            (Station::Chrome, "refresh_chrome"),
            (Station::WebSpoke, "drive_web_page"),
        ] {
            assert_eq!(station.label(), label);
            assert_eq!(
                Station::from_byte(u8::try_from(station.slot()).expect("one byte")),
                station,
                "{label} does not come back out of the ledger it goes into",
            );
        }
    }

    /// RED (43) — **the stall self-report has a word for every phase of a web
    /// page coming up.**
    ///
    /// The two holds of 2026-09-23 while a web preview opened read
    /// `window_event 3979 ms` and `window_event 2703 ms` with every named child
    /// under 130 ms: the environment request, the controller request, the
    /// install burst's parts, the first navigation and the pump the engine's
    /// callbacks arrive on had no word in this vocabulary, so a four-second
    /// stall could only be called an event. Each word is the function a reader
    /// greps for, and each comes back out of the ledger it goes into.
    ///
    /// MUTATION: drop `Self::WebInstall => "WebHost::install"` from
    /// [`Station::label`] (or the variant) and the install burst has no word.
    #[test]
    fn the_stall_report_has_a_word_for_every_phase_of_a_web_page_coming_up() {
        let vocabulary: Vec<&'static str> = (0..STATION_COUNT)
            .map(|slot| Station::from_byte(u8::try_from(slot).expect("one byte")).label())
            .collect();
        for word in [
            "request_environment",
            "request_controller",
            "attach_web_visual",
            "WebHost::install",
            "stand_on_the_floor",
            "WebHost::navigate",
            "message pump",
        ] {
            assert!(
                vocabulary.contains(&word),
                "`{word}` is not a word the stall self-report can say"
            );
        }
    }

    /// RED (43) — **a hold spent bringing a web page up says which phase spent
    /// it**, and the pump the engine's callbacks run on is not charged to the
    /// event that had already returned.
    ///
    /// The synthetic turn is the shape of the 2026-09-23 report: a press that
    /// opens the page, the thread handed back to the pump, the environment's
    /// answer read on its own wake, and the controller's answer installed and
    /// navigated. Before ticket 43 the pump's three seconds went to
    /// `window_event` and the install burst to `drive_web_page`.
    ///
    /// MUTATION: map `Station::Pump` to the label `window_event` shares and the
    /// line reads `window_event 3000 ms` again (and
    /// `every_station_prints_its_own_word` goes red with it).
    #[test]
    fn a_hold_spent_bringing_a_web_page_up_says_which_phase_spent_it() {
        let heart = Heartbeat::sampling(no_footprint);
        heart.woke_at(1_000);
        heart.at_station(Station::Event, 1_000);
        let press = heart.enter_at(Station::EventMouse, 0, 1_000);
        let environment = heart.enter_at(Station::WebEnvironment, 0, 1_010);
        if let Location::Resume {
            station,
            node,
            scope,
        } = environment
        {
            heart.resume_at(station, node, scope, 1_090);
        }
        if let Location::Resume {
            station,
            node,
            scope,
        } = press
        {
            heart.resume_at(station, node, scope, 1_100);
        }
        heart.at_station(Station::Pump, 1_100);
        heart.at_station(Station::WebSpoke, 4_100);
        heart.at_station(Station::WebController, 4_110);
        heart.at_station(Station::WebVisual, 4_150);
        heart.at_station(Station::WebInstall, 4_160);
        heart.at_station(Station::WebFloor, 4_260);
        heart.at_station(Station::WebNavigate, 4_280);
        heart.at_station(Station::Pump, 4_300);
        heart.park_at(Park::Indefinite, 4_300);
        let (holds, dropped) = heart.take_slow_holds();
        assert_eq!(dropped, 0);
        let [hold] = holds.as_slice() else {
            panic!("one hold, and it ran long: {holds:?}")
        };
        assert_eq!(hold.spent_ms[Station::Pump.slot()], 3_000);
        assert_eq!(hold.spent_ms[Station::Event.slot()], 0);
        assert_eq!(hold.spent_ms[Station::WebInstall.slot()], 100);
        let line = hold.line();
        for named in [
            "message pump 3000 ms",
            "WebHost::install 100 ms",
            "request_environment 80 ms",
            "request_controller 40 ms",
            "stand_on_the_floor 20 ms",
            "WebHost::navigate 20 ms",
            "attach_web_visual 10 ms",
        ] {
            assert!(
                line.contains(named),
                "`{named}` is not in the line:\n{line}"
            );
        }
        assert!(
            line.contains("window_event 0 ms (mouse_input 20 ms (request_environment 80 ms))"),
            "the pump's time is charged to an event that had returned, or the \
             environment request is not the gesture's own child:\n{line}"
        );
    }

    /// **The ledger is emptied between holds**, or the next slow line would be
    /// this one's arithmetic said twice.
    #[test]
    fn the_ledger_is_emptied_between_holds() {
        let heart = Heartbeat::sampling(no_footprint);
        heart.woke_at(0);
        heart.at_station(Station::WebPage, 10);
        heart.park_at(Park::Indefinite, 3_000);
        assert_eq!(heart.take_slow_holds().0.len(), 1);
        heart.woke_at(4_000);
        heart.at_station(Station::Drain, 4_005);
        heart.park_at(Park::Indefinite, 4_020);
        assert_eq!(
            heart.take_slow_holds(),
            (Vec::new(), 0),
            "the short hold after a long one carries none of its milliseconds"
        );
    }

    /// Session age and the ordinal come from the heartbeat that owns the
    /// ledger, and the ordinal advances only for a line admitted to its queue.
    #[test]
    fn slow_hold_lines_carry_session_age_and_a_running_count() {
        let heart = Heartbeat::sampling(no_footprint);
        heart.woke_at(1_000);
        heart.park_at(Park::Indefinite, 2_000);
        heart.woke_at(9_000);
        heart.park_at(Park::Indefinite, 10_000);
        let (holds, dropped) = heart.take_slow_holds();
        assert_eq!(dropped, 0);
        assert_eq!(
            holds
                .iter()
                .map(|hold| (hold.session_age_ms, hold.stall_count))
                .collect::<Vec<_>>(),
            [(2_000, 1), (10_000, 2)],
        );
    }

    /// **Parking twice with no wake between them is one hold, not two.** The
    /// turn's body leaves early in six places and every one of them parks.
    #[test]
    fn a_second_park_with_no_wake_between_records_nothing() {
        let heart = Heartbeat::sampling(no_footprint);
        heart.woke_at(0);
        heart.at_station(Station::WebPage, 10);
        heart.park_at(Park::Indefinite, 2_000);
        heart.park_at(Park::Indefinite, 9_000);
        let (slow, _) = heart.take_slow_holds();
        assert_eq!(slow.len(), 1, "one hold was open, so one hold is written");
        assert_eq!(slow[0].held_ms, 2_000);
    }

    /// **A turn reached with no wake before it opens its own hold.** The run's
    /// first turn is exactly that, and a hold measured from an origin belonging
    /// to nothing would file the whole of startup as a stall.
    #[test]
    fn a_turn_with_no_wake_before_it_opens_its_own_hold() {
        let heart = Heartbeat::sampling(no_footprint);
        heart.beat_at(8_000);
        heart.at_station(Station::Drain, 8_010);
        heart.park_at(Park::Indefinite, 8_030);
        assert_eq!(
            heart.take_slow_holds(),
            (Vec::new(), 0),
            "thirty milliseconds is thirty milliseconds, not eight seconds"
        );
    }

    /// **The threshold is the one this module states**, read from the constant
    /// rather than from a number written down a second time.
    #[test]
    fn the_threshold_is_the_one_the_module_states() {
        let heart = Heartbeat::sampling(no_footprint);
        let bound = slow_hold_threshold_ms();
        heart.woke_at(0);
        heart.park_at(Park::Indefinite, bound - 1);
        assert_eq!(heart.take_slow_holds().0, Vec::new(), "one short of it");
        heart.woke_at(10_000);
        heart.park_at(Park::Indefinite, 10_000 + bound);
        assert_eq!(heart.take_slow_holds().0.len(), 1, "exactly it");
    }

    /// **Every station has a slot in the ledger**, which is the whole of what
    /// `STATION_COUNT` promises: a thirteenth variant added without widening
    /// the array would charge its milliseconds to nobody, and the line would
    /// quietly stop adding up.
    #[test]
    fn every_station_has_a_slot_in_the_ledger() {
        let mut seen = Vec::new();
        for slot in 0..STATION_COUNT {
            let station = Station::from_byte(u8::try_from(slot).expect("a slot is one byte"));
            assert_eq!(
                station.slot(),
                slot,
                "{station} answers a slot it is not at"
            );
            seen.push(station);
        }
        seen.dedup();
        assert_eq!(seen.len(), STATION_COUNT, "two stations share one slot");
        assert_eq!(
            Station::from_byte(u8::try_from(STATION_COUNT).expect("one byte")),
            Station::Starting,
            "a station past the ledger's width would be charged to `starting`",
        );
    }

    /// **No two stations print the same word** (T-STATION-SPLIT).
    ///
    /// The line is read as a list of lanes and their milliseconds, so two lanes
    /// answering to one word would be a reader adding up two numbers that are
    /// about different work — and the slice that split one span into seven is
    /// exactly the kind of change that can reach for a word already taken.
    #[test]
    fn every_station_prints_its_own_word() {
        let mut words: Vec<&'static str> = Vec::new();
        for slot in 0..STATION_COUNT {
            let station = Station::from_byte(u8::try_from(slot).expect("a slot is one byte"));
            words.push(station.label());
        }
        let spoken = words.len();
        words.sort_unstable();
        words.dedup();
        assert_eq!(words.len(), spoken, "two stations print the same word");
    }

    /// A hold whose stations all rounded to nothing still states its length.
    #[test]
    fn a_hold_with_no_named_station_still_states_its_length() {
        let hold = SlowHold {
            cpu_us: None,
            detail: super::detail::Tree::default(),
            turn: 7,
            held_ms: 900,
            session_age_ms: 12_345,
            stall_count: 4,
            spent_ms: [0; STATION_COUNT],
            paging: None,
        };
        assert_eq!(
            hold.line(),
            "Folio: the window thread held control for 900 ms on turn 7 — no station held it · \
             session age 12.345s, stall #4",
        );
    }

    // ══ Whose seconds they were: the footprint at the two ends of a hold ══

    /// **The line says what the memory manager did**, in the shape the ticket
    /// fixed: everything that was there before, then a middle dot, then the
    /// faults as a difference and the working set as a movement.
    ///
    /// Pure — a `SlowHold` and nothing else — which is the same rule
    /// [`SlowHold::line`] is written to and the reason the shape of a log line
    /// is a thing this repository can state without running a program.
    #[test]
    fn a_slow_holds_line_states_what_the_machine_did_to_its_memory() {
        let mut spent_ms = [0; STATION_COUNT];
        spent_ms[Station::Present.slot()] = 2_092;
        spent_ms[Station::Wheel.slot()] = 1_928;
        let hold = SlowHold {
            cpu_us: None,
            detail: super::detail::Tree::default(),
            turn: 3_937_579,
            held_ms: 4_056,
            session_age_ms: 8_404_000,
            stall_count: 31,
            spent_ms,
            paging: Some(Paging {
                faults: 38_210,
                working_set_before: 179 * 1024 * 1024,
                working_set_after: 412 * 1024 * 1024,
            }),
        };
        assert_eq!(
            hold.line(),
            "Folio: the window thread held control for 4056 ms on turn 3937579 — \
             publish_frame_inner 2092 ms, flush_wheel 1928 ms · \
             faults +38210, working set 179 → 412 MB · \
             session age 8404.000s, stall #31",
        );
    }

    /// **A hold nobody could sample prints exactly the line it always printed.**
    ///
    /// The grep the whole facility is read through is `held control for`, and
    /// the stations after it are read by eye; a platform with no counters, or a
    /// call the kernel refused, must not move either. This is the assertion that
    /// would go red if the new fields were ever put in the middle of the line or
    /// printed as zeroes when there was nothing to print.
    #[test]
    fn a_hold_with_no_footprint_prints_the_line_it_always_printed() {
        let mut spent_ms = [0; STATION_COUNT];
        spent_ms[Station::Wheel.slot()] = 1_928;
        let hold = SlowHold {
            cpu_us: None,
            detail: super::detail::Tree::default(),
            turn: 3_937_579,
            held_ms: 4_056,
            session_age_ms: 8_404_000,
            stall_count: 31,
            spent_ms,
            paging: None,
        };
        assert_eq!(
            hold.line(),
            "Folio: the window thread held control for 4056 ms on turn 3937579 — \
             flush_wheel 1928 ms · session age 8404.000s, stall #31",
        );
    }

    /// **Megabytes are rounded to the nearest, and they are the ones Task
    /// Manager prints** — 1024-based, which is the whole reason this is a
    /// function and not an inline division somebody would write the other way
    /// the second time.
    #[test]
    fn a_working_set_is_printed_in_the_megabytes_a_reader_recognises() {
        let hold = |before: u64, after: u64| SlowHold {
            cpu_us: None,
            detail: super::detail::Tree::default(),
            turn: 0,
            held_ms: 900,
            session_age_ms: 900,
            stall_count: 1,
            spent_ms: [0; STATION_COUNT],
            paging: Some(Paging {
                faults: 0,
                working_set_before: before,
                working_set_after: after,
            }),
        };
        // Half a megabyte short of 180 rounds up; a hair over 179 stays.
        let before = 180 * 1024 * 1024 - 512 * 1024;
        let after = 179 * 1024 * 1024 + 1;
        let line = hold(before, after).line();
        assert!(
            line.ends_with("· faults +0, working set 180 → 179 MB · session age 0.900s, stall #1"),
            "rounded to the nearest, both ends: {line}"
        );
        assert!(
            hold(0, 0)
                .line()
                .ends_with("working set 0 → 0 MB · session age 0.900s, stall #1"),
            "nothing resident is nothing, not a division that trapped"
        );
    }

    /// **The counters are sampled at the two ends of the hold**, so the number
    /// the line carries is the hold's own and not the run's.
    ///
    /// The fake sampler is handed two readings: one for the wake that opens the
    /// hold, one for the park that reports it. What the line prints is their
    /// difference and their pair — an instrument that printed the closing
    /// reading alone would print this process's lifetime fault count, which is
    /// in the millions and says nothing about any hold at all.
    #[test]
    fn a_slow_hold_carries_the_faults_taken_between_its_own_two_ends() {
        queue_footprints(&[
            (1_000_000, 179 * 1024 * 1024),
            (1_038_210, 412 * 1024 * 1024),
        ]);
        let heart = Heartbeat::sampling(fake_footprint);
        heart.woke_at(1_000);
        heart.at_station(Station::Wheel, 1_010);
        heart.park_at(Park::Indefinite, 2_900);
        let (slow, _) = heart.take_slow_holds();
        let [hold] = slow.as_slice() else {
            panic!("one hold ran long: {slow:?}")
        };
        assert_eq!(
            hold.paging,
            Some(Paging {
                faults: 38_210,
                working_set_before: 179 * 1024 * 1024,
                working_set_after: 412 * 1024 * 1024,
            }),
            "the difference across the hold, not the process's running total",
        );
        assert_eq!(footprints_asked(), 2, "one end, then the other");
        assert!(
            hold.line().ends_with(
                "· faults +38210, working set 179 → 412 MB · session age 2.900s, stall #1"
            ),
            "{}",
            hold.line(),
        );
    }

    /// **An ordinary hold refreshes the coarse sample when it is old and never
    /// asks on the reporting path** — the cost rule, stated as a number rather
    /// than as a comment.
    ///
    /// The first hold pays the opening query; rapid followers reuse it. What an
    /// ordinary hold never pays is the second query: it lives past the threshold
    /// check, on the path that produces a line.
    ///
    /// MUTATION: move `close_footprint` above that check and this reads 2.
    #[test]
    fn an_ordinary_hold_asks_the_sampler_once_and_the_reporting_path_twice() {
        queue_footprints(&[(10, 1024), (20, 2048), (30, 4096)]);
        let heart = Heartbeat::sampling(fake_footprint);
        heart.woke_at(1_000);
        heart.at_station(Station::Present, 1_004);
        heart.park_at(Park::Indefinite, 1_012);
        assert_eq!(heart.take_slow_holds(), (Vec::new(), 0));
        assert_eq!(
            footprints_asked(),
            1,
            "a hold nobody writes down takes one sample and stops",
        );
        let bound = slow_hold_threshold_ms();
        heart.woke_at(2_000);
        heart.park_at(Park::Indefinite, 2_000 + bound);
        assert_eq!(heart.take_slow_holds().0.len(), 1);
        assert_eq!(
            footprints_asked(),
            3,
            "the slow one opened with a sample and closed with a second",
        );
    }

    /// A busy or spuriously woken run loop does not turn the diagnostic into
    /// another wake-time platform call. The boundary itself refreshes so the
    /// baseline used by a report is never older than the advertised interval.
    #[test]
    fn rapid_holds_share_one_coarse_opening_sample() {
        queue_footprints(&[(10, 1024), (20, 2048)]);
        let heart = Heartbeat::sampling(fake_footprint);
        for now_ms in [1_000, 1_010, 1_100, 1_249] {
            heart.woke_at(now_ms);
            heart.park_at(Park::Indefinite, now_ms + 1);
        }
        assert_eq!(
            footprints_asked(),
            1,
            "the cached sample covers the interval"
        );

        heart.woke_at(1_250);
        heart.park_at(Park::Indefinite, 1_251);
        assert_eq!(footprints_asked(), 2, "the boundary refreshes the baseline");
    }

    /// **A platform that counts nothing says nothing**, and a hold whose
    /// opening sample was refused does not print the closing one on its own.
    ///
    /// Two different silences, and they have to read the same in the log: half
    /// a movement printed as a whole one is the kind of number a reader would
    /// act on.
    #[test]
    fn a_hold_whose_first_sample_was_refused_prints_no_counters() {
        // Nothing queued, so the opening sample is refused; the second entry
        // would be answered if anything asked for it.
        queue_footprints(&[]);
        let heart = Heartbeat::sampling(fake_footprint);
        heart.woke_at(0);
        heart.at_station(Station::Drain, 10);
        heart.park_at(Park::Indefinite, 3_000);
        let (slow, _) = heart.take_slow_holds();
        let [hold] = slow.as_slice() else {
            panic!("one hold ran long: {slow:?}")
        };
        assert_eq!(hold.paging, None);
        assert_eq!(
            footprints_asked(),
            1,
            "with no baseline there is nothing a second sample could be \
             subtracted from, so it is not taken",
        );
        assert_eq!(
            hold.line(),
            "Folio: the window thread held control for 3000 ms on turn 0 — \
             drain_pty 2990 ms, woken 10 ms · session age 3.000s, stall #1",
        );
    }

    /// **The baseline belongs to the hold in progress, not to the one before
    /// it.** A run of holds each open with their own sample, so a fault taken
    /// while the thread was parked is charged to nobody.
    #[test]
    fn each_hold_opens_with_its_own_baseline() {
        queue_footprints(&[
            // The short hold's baseline, which is the only sample it takes,
            (100, 1024 * 1024),
            // then the long hold's two ends.
            (500, 2 * 1024 * 1024),
            (700, 3 * 1024 * 1024),
        ]);
        let heart = Heartbeat::sampling(fake_footprint);
        heart.woke_at(0);
        heart.park_at(Park::Indefinite, 10);
        assert_eq!(heart.take_slow_holds(), (Vec::new(), 0));
        heart.woke_at(1_000);
        heart.park_at(Park::Indefinite, 4_000);
        let (slow, _) = heart.take_slow_holds();
        let [hold] = slow.as_slice() else {
            panic!("the second hold ran long: {slow:?}")
        };
        assert_eq!(
            hold.paging.map(|paging| paging.faults),
            Some(200),
            "the 400 faults taken across the first hold and the park after it \
             are not this hold's",
        );
    }
}
