//! **What the window thread was doing at the moment it stopped answering.**
//!
//! A third unsafe boundary in this crate, against a third thing: `windows_impl`
//! is Win32 through the `windows` crate for the window's own sake, [`webview`]
//! is WebView2 through `webview2-com`, and this is Win32 turned on **our own
//! process** for the sake of a report nobody will be present to take.
//!
//! # Why a stack and not a log line
//!
//! The hangs this exists for are intermittent and unreproducible — a white
//! frame and `Not Responding`, minutes apart, on a build that has already had
//! its two known livelocks fixed (a `drain_leaf_pty` that never returned, an
//! unbounded PTY write). What is left is by definition something nobody has
//! named yet, and the only artefact that can name it is **the return addresses
//! that were on the window thread's stack while it was stuck**. `bt-app`'s
//! watchdog decides *when* to ask; this module is the asking.
//!
//! # The order of operations is the whole design
//!
//! Sampling a suspended thread from inside the same process is a deadlock
//! waiting to be written, because the suspended thread is holding locks that
//! the sampler is about to want. There are two that matter and both are avoided
//! by **doing the work before the suspend, not during it**:
//!
//! - **The loader lock.** `EnumProcessModulesEx` walks the module list, which
//!   the loader owns. If the window thread were suspended inside `LoadLibrary`
//!   — which a WebView2 call can perfectly well be — the enumeration would
//!   block forever, on the watchdog thread, with the UI thread suspended: a
//!   hang report that *causes* a permanent hang. So the module map is built
//!   first and the suspend happens after it.
//! - **The heap lock.** Any allocation can block on the CRT heap, which the
//!   suspended thread may hold. So the stack buffer is allocated first too, and
//!   between [`SuspendThread`] and [`ResumeThread`] this module allocates
//!   nothing, formats nothing and takes no lock of its own — it makes exactly
//!   two calls, [`GetThreadContext`] and [`ReadProcessMemory`], both of which
//!   are satisfied by the kernel without touching user-mode state.
//!
//! Symbolisation and formatting happen after the resume, off the sample.
//!
//! # Why a scan and not a walk
//!
//! `RtlCaptureStackBackTrace` samples the *calling* thread and cannot be
//! pointed at another one, so it is not available here. `StackWalk64` can, but
//! it lives in `dbghelp.dll`, is single-threaded-by-contract, wants symbols this
//! build does not ship, and is exactly the kind of machinery one does not want
//! to invoke while another thread of the same process is suspended.
//!
//! What is left is what a debugger does when it has no unwind information: take
//! `rip`, then **read the raw stack and keep every qword that lands inside a
//! loaded module**. It over-reports — stale return addresses from frames that
//! have already returned stay on the stack until they are overwritten — and it
//! is deliberately not filtered, because a filter clever enough to drop the
//! stale ones is clever enough to drop the one that mattered. A reader gets
//! `rip` (which is exact) followed by a depth-ordered list of candidates, and
//! the module names alone answer the question this is for: *terminal code,
//! ConPTY, WebView2, or the kernel?*
//!
//! This is the in-process form of the `hangprobe.ps1` that found the drain
//! livelock from outside; the difference is that it no longer needs a human to
//! be watching at the moment it happens.
//!
//! # And on a Mac, where two of the three halves are somebody else's (M4-11)
//!
//! The question — [`ask_thread_to_answer`] — has a real arm there, because it
//! is the one this module is really built on: the watchdog convicts nobody
//! until a thread has been asked and has stayed quiet. The *answering* is
//! [`CFRunLoopPerformBlock`] on the main run loop in `kCFRunLoopCommonModes`,
//! which is the same measurement `WM_NULL` is and not the same measurement
//! winit's user events are — see that function's own note.
//!
//! The **sample** is not ported and that is a decision, not an omission.
//! Suspending a thread of one's own process and reading its stack is a Windows
//! facility this module gets to use because Win32 hands out `SuspendThread` and
//! `ReadProcessMemory`; the Mach twins are `thread_suspend` and `vm_read`, and
//! reaching for them would be this program writing a debugger against itself on
//! a platform that already **writes the report for it**. macOS's own
//! `ReportCrash` files a fully-formed `.ips` into
//! `~/Library/Logs/DiagnosticReports/` for every process that dies of a signal,
//! with every thread's backtrace in it — so the macOS half of the crash story
//! is [`system_crash_reports_directory`] and the next launch naming what it
//! finds there (`bt_app::diagnostics`), rather than a second-rate copy of a
//! stack the system already took.
//!
//! [`CFRunLoopPerformBlock`]: https://developer.apple.com/documentation/corefoundation/1543030-cfrunloopperformblock

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// How much of the stack is read and scanned for return addresses.
///
/// 128 KiB because a default Windows thread stack reserves 1 MiB and commits
/// far less; a `ReadProcessMemory` that runs off the committed end fails as a
/// whole rather than partially, which is what the halving in [`read_stack`] is
/// for. Deep enough to reach past a WebView2 message pump, small enough that
/// the read is a memcpy and not an event.
///
/// `#[cfg(windows)]` with its one reader: `read_stack` suspends a thread and
/// reads its stack through `ReadProcessMemory`, which is the facility this
/// module's own header says is not reached for off Windows — M4-11 collects the
/// system's crash reports there instead.
#[cfg(windows)]
const STACK_SCAN_BYTES: usize = 128 * 1024;

/// One address, named by the module it fell in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleSite {
    pub address: u64,
    pub module: String,
    pub offset: u64,
    /// Distance in bytes from `rsp`, so a reader can tell an outer frame from an
    /// inner one. Zero for `rip`, which is not on the stack at all.
    pub depth: usize,
}

impl fmt::Display for ModuleSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}+0x{:x} (0x{:016x})",
            self.module, self.offset, self.address
        )
    }
}

/// One look at another thread's stack.
///
/// Every field is best-effort and `note` says which effort failed, because a
/// report that says "GetThreadContext was refused" is still evidence and a
/// report that says nothing is not.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StackSample {
    pub rip: u64,
    pub rsp: u64,
    /// `rip` resolved, when it fell inside a loaded module.
    pub rip_site: Option<ModuleSite>,
    /// Every module-resolvable qword found on the stack, innermost first.
    pub frames: Vec<ModuleSite>,
    /// How many bytes of stack the read actually returned.
    pub scanned_bytes: usize,
    /// How many modules the map held. Zero means the map itself failed, which
    /// makes an empty `frames` mean "could not resolve" rather than "nothing
    /// there".
    pub modules: usize,
    pub note: Option<&'static str>,
}

impl StackSample {
    fn refused(note: &'static str) -> Self {
        Self {
            note: Some(note),
            ..Self::default()
        }
    }
}

/// The calling thread's id, which is the handle a sampler can be given.
///
/// A thread **id** and not a `HANDLE`: the window thread hands this over at
/// startup and the watchdog opens the thread itself, on its own thread, so no
/// raw handle ever crosses a thread boundary and there is no lifetime to get
/// wrong. A `u32` is also a plain `Send` value, which a `HANDLE` is not.
#[cfg(windows)]
#[must_use]
pub fn current_thread_id() -> u32 {
    // A pure read of the calling thread's own id. It cannot fail and has no
    // handle to release.
    unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
}

/// The calling thread's Mach port name — and, if this is the main thread, the
/// one moment this module learns which port that is.
///
/// **The id.** `pthread_mach_thread_np` is the `GetCurrentThreadId` of this
/// platform: a `mach_port_t`, which is already a `u32`, stable for the life of
/// the thread, unique within the task, and the number every Mach-level tool
/// names a thread by. It borrows no reference — unlike `mach_thread_self`,
/// which returns a *send right* the caller then owes a `mach_port_deallocate`
/// — so this stays what the Windows arm is: a pure read with nothing to
/// release.
///
/// **The registration, and why it belongs in this function rather than in a
/// second one.** On macOS there is exactly one thread whose run loop AppKit
/// turns and on which every window of this process lives, and
/// [`ask_thread_to_answer`] — called from the watchdog thread — has to be able
/// to tell whether the id it was handed is that thread. Nothing in Mach maps a
/// port back to "is this the main thread" from *another* thread;
/// `pthread_main_np` answers only for the caller. So the answer is recorded at
/// the one moment a caller is in a position to give it: when the main thread
/// asks for its own id, which is precisely what the window thread does at
/// startup to hand it to the watchdog (`bt_app::hang_watch::start`).
///
/// A thread that is not the main one writes nothing, so the port recorded here
/// is the main thread's or it is absent, and absent is answered as
/// [`Answer::NoWindow`] rather than guessed at.
#[cfg(target_os = "macos")]
#[must_use]
pub fn current_thread_id() -> u32 {
    // SAFETY: two `pthread` reads about the calling thread. Neither takes a
    // reference, allocates, or can fail.
    let (id, is_main) = unsafe {
        (
            libc::pthread_mach_thread_np(libc::pthread_self()),
            libc::pthread_main_np() != 0,
        )
    };
    if is_main {
        MAIN_THREAD_PORT.store(id, std::sync::atomic::Ordering::Relaxed);
    }
    id
}

/// The main thread's Mach port, or zero while no main thread has identified
/// itself. Written once, by [`current_thread_id`]; read by
/// [`ask_thread_to_answer`] from the watchdog thread.
///
/// Zero is not a valid port name (`MACH_PORT_NULL`), so it needs no second flag
/// to mean "nothing has been recorded".
#[cfg(target_os = "macos")]
static MAIN_THREAD_PORT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(all(not(windows), not(target_os = "macos")))]
#[must_use]
pub fn current_thread_id() -> u32 {
    0
}

/// **What a thread said when it was asked whether it is still there.**
///
/// The three answers are three different facts and a watchdog needs all of
/// them: a thread that replies is alive whatever else it is doing, a thread
/// that is asked and stays silent is the fault this facility exists for, and a
/// thread with no window to ask is neither — it is a question that could not be
/// put, which before the first window and after the last one is the ordinary
/// state of affairs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Answer {
    /// It answered inside the timeout. It is servicing messages.
    Answered,
    /// It was asked and it did not answer — or Windows already holds the window
    /// hung and refused to wait at all. On macOS: the main run loop did not
    /// reach the block inside the bound.
    Silent,
    /// There was no window on that thread to put the question to. On macOS that
    /// is every thread but the main one, which is the only one a window can be
    /// on — and the state before any main thread has identified itself.
    NoWindow,
}

impl Answer {
    /// The sentence a report prints for this answer.
    #[must_use]
    pub fn phrase(self) -> &'static str {
        match self {
            Self::Answered => "the window answered, so the thread is alive",
            Self::Silent => "the window was asked and did not answer",
            Self::NoWindow => "this thread owned no window to ask",
        }
    }
}

/// **Ask the thread whether it is still answering, and wait a bounded time.**
///
/// The question this asks is not "is it busy" but "does it still service what
/// is sent to it", and those are different facts about a Windows UI thread. A
/// thread inside a USER32 modal loop — the drag that resizes a window, a
/// tracked menu — is not turning its own event loop at all, and yet it is
/// perfectly alive: it is pumping, it repaints, it answers. A watchdog that
/// judged on the loop alone would file a report on every window somebody
/// dragged for six seconds.
///
/// `WM_NULL` because it is the message that means nothing: no window procedure
/// in this process or any library in it can act on it, so the question cannot
/// change what it is measuring. `SendMessageTimeout` because the whole value is
/// in the bound — a plain `SendMessage` to a wedged thread would wedge the
/// watchdog too, which is the one thing a watchdog may never do.
/// `SMTO_ABORTIFHUNG` on top of the timeout, so that a window Windows has
/// *already* decided is hung answers immediately rather than after the full
/// wait: that is the same fact arriving a second earlier.
///
/// The window is found by enumeration rather than being handed over at startup,
/// because the thread's windows come and go — there is none before the event
/// loop is built, several while the product is in use, and none again at the
/// end — and a watchdog holding one `HWND` from birth would be asking about a
/// window that has been destroyed.
#[cfg(windows)]
#[must_use]
pub fn ask_thread_to_answer(thread_id: u32, timeout: Duration) -> Answer {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, SMTO_ABORTIFHUNG, SMTO_BLOCK, SendMessageTimeoutW, WM_NULL,
    };

    /// Keep the first window and stop. `state` is the `&mut HWND` below, which
    /// outlives the enumeration because `EnumThreadWindows` is synchronous and
    /// returns before the borrow ends.
    unsafe extern "system" fn keep_the_first(window: HWND, state: LPARAM) -> windows::core::BOOL {
        unsafe { *(state.0 as *mut HWND) = window };
        // `FALSE` stops the enumeration: any one window of the thread is as good
        // a question as any other, because they all answer out of the same pump.
        false.into()
    }

    let mut window = HWND::default();
    let state = LPARAM((&raw mut window) as isize);
    // The answer is "did the callback run to the end", which is `FALSE` on every
    // thread that has a window — the callback stops at the first one. What
    // matters is whether `window` was filled in, and that is the test below.
    let _ = unsafe { EnumThreadWindows(thread_id, Some(keep_the_first), state) };
    if window.is_invalid() {
        return Answer::NoWindow;
    }
    let milliseconds = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    let mut reply = 0_usize;
    // Non-zero is success; zero is "the timeout elapsed" or "already hung".
    let answered = unsafe {
        SendMessageTimeoutW(
            window,
            WM_NULL,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            milliseconds,
            Some(&raw mut reply),
        )
    };
    if answered.0 == 0 {
        Answer::Silent
    } else {
        Answer::Answered
    }
}

/// **Ask the main run loop whether it is still turning, and wait a bounded
/// time** (M4-11).
///
/// # Which of the two things on offer this measures, and why
///
/// The Windows arm's subject is not "did the application's loop come round".
/// `SendMessageTimeout(WM_NULL)` is dispatched by **USER32**, below anything
/// this program wrote: a window thread inside a modal drag or a tracked menu is
/// not turning winit's loop at all and still answers, which is the whole reason
/// that function's own note gives for choosing it. There were two candidates
/// here and only one of them is that same measurement:
///
/// * **winit's user event through the proxy `bt-app` already holds.** It is
///   answered by `bt_app`'s own `user_event` handler — which is the same
///   machinery whose silence raised the suspicion in the first place. A
///   last-resort question that only the suspect can answer adds no evidence: it
///   would convict a main thread that is busy inside one of Folio's own
///   handlers, and `hang_watch`'s heartbeat already measures exactly that.
/// * **`CFRunLoopPerformBlock` on the main run loop, in
///   `kCFRunLoopCommonModes`.** The block is performed by Core Foundation
///   itself, at the top of the loop, before anything of ours is consulted —
///   which is `WM_NULL`'s position exactly. And the mode is the one that makes
///   it the *same* answer under a nested loop: AppKit puts
///   `NSEventTrackingRunLoopMode` and `NSModalPanelRunLoopMode` into the common
///   set, so a window being dragged, a menu being tracked or a sheet being
///   answered still performs it, for the reason a USER32 modal loop still
///   pumps. `kCFRunLoopDefaultMode` would have been the other spelling and it
///   is the wrong one: it reports every drag as a hang.
///
/// So this is the second, and `wake_up` after it because a run loop asleep in
/// `mach_msg` has to be told there is now something to do — the block alone
/// would be answered whenever the reader next moved the mouse, which is a
/// measurement of the reader.
///
/// # Why an id is still taken, on a platform with one loop
///
/// Because it is still a question about a *thread*, and only one thread on this
/// platform has a run loop anybody else drives. An id that is not the main
/// thread's is [`Answer::NoWindow`] — "this thread owned no window to ask" is
/// literally true of every other thread in a Cocoa process — and so is the
/// state before any main thread has identified itself. See
/// [`current_thread_id`] for where that identity is recorded.
///
/// # The wait, and what is left behind by a wait that fails
///
/// A bounded channel, because the whole value is in the bound: the watchdog may
/// never block on the thread it is watching. A `Silent` answer leaves the block
/// queued on a loop that is not turning, holding the sending half; if the loop
/// comes back it runs, sends into a receiver that is gone, and is released.
/// That is one block per threshold on a process that is already hung, and
/// nothing at all on one that is not.
#[cfg(target_os = "macos")]
#[must_use]
pub fn ask_thread_to_answer(thread_id: u32, timeout: Duration) -> Answer {
    let main = MAIN_THREAD_PORT.load(std::sync::atomic::Ordering::Relaxed);
    if main == 0 || thread_id != main {
        return Answer::NoWindow;
    }
    let Some(run_loop) = objc2_core_foundation::CFRunLoop::main() else {
        return Answer::NoWindow;
    };
    ask_run_loop_to_answer(&run_loop, timeout)
}

/// The half of [`ask_thread_to_answer`] that is about a run loop rather than
/// about which thread owns it — split out because it is the half that can be
/// tested, against a loop this process makes and turns on purpose.
#[cfg(target_os = "macos")]
fn ask_run_loop_to_answer(
    run_loop: &objc2_core_foundation::CFRunLoop,
    timeout: Duration,
) -> Answer {
    use objc2_core_foundation::CFType;

    let (answered, hear) = std::sync::mpsc::sync_channel::<()>(1);
    // `try_send` and not `send`: the block may be performed after the wait has
    // already given up, and a watchdog's question must never be able to park
    // the thread it is asking.
    let block = block2::RcBlock::new(move || {
        let _ = answered.try_send(());
    });
    // SAFETY: `kCFRunLoopCommonModes` is a Core Foundation constant, initialised
    // before any run loop exists and never written.
    let Some(common_modes) = (unsafe { objc2_core_foundation::kCFRunLoopCommonModes }) else {
        return Answer::NoWindow;
    };
    let common_modes: &CFType = common_modes;
    // SAFETY: `CFRunLoopPerformBlock` is one of the three calls Core Foundation
    // documents as safe to make on *another* thread's run loop (with
    // `CFRunLoopWakeUp` and `CFRunLoopAddSource`), which is the whole reason the
    // watchdog may ask at all. The mode is the common-modes constant, which is
    // the `CFString` this parameter is declared in terms of, and the block is a
    // live `RcBlock` that Core Foundation copies — and therefore retains —
    // before this call returns.
    unsafe { run_loop.perform_block(Some(common_modes), Some(&block)) };
    run_loop.wake_up();
    match hear.recv_timeout(timeout) {
        Ok(()) => Answer::Answered,
        // Both failures are the same fact: nothing performed the block inside
        // the bound. `Disconnected` is the block being released unperformed,
        // which is a run loop that went away without turning.
        Err(_) => Answer::Silent,
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
#[must_use]
pub fn ask_thread_to_answer(_thread_id: u32, _timeout: Duration) -> Answer {
    Answer::NoWindow
}

/// **Where this machine files the crash reports it writes for us**, or `None`
/// on a platform that files none (M4-11).
///
/// # macOS
///
/// `~/Library/Logs/DiagnosticReports/`, which is where `ReportCrash` puts the
/// `.ips` it writes for every process of this user that dies of a signal — the
/// `kill -ABRT` in M4's acceptance line ⑥, and equally the `EXC_BAD_ACCESS`
/// nobody meant. The report already holds every thread's backtrace, which is
/// why [`capture_thread_stack`] is not ported: the system took the sample this
/// module would have had to suspend a thread for.
///
/// **The account's home and not `$HOME`, and that is the correctness of this
/// function.** The report is not written by this process; it is written by a
/// system service that knows the *account*. A Folio handed a different `HOME` —
/// which is how every probe in this workspace isolates itself, and how a user
/// relocates the storage directory (`bt_app::persist::storage_dir`) — would
/// otherwise look in a directory `ReportCrash` never writes to and quietly
/// never find anything. `getpwuid` is read once per process, for its `pw_dir`,
/// and nothing else in the record is touched.
///
/// # Windows, and everything else
///
/// `None`, and not as a stand-in for work not done: Windows files nothing here
/// to find. Windows Error Reporting writes a user-mode dump only when
/// `LocalDumps` has been configured in the registry, which is off on every
/// machine Folio ships to, and what it would write is not a per-application
/// directory this program may read. The Windows half of the same story is the
/// hang report this module's own sample feeds and the panic log the hook
/// writes, both of which are Folio's own files.
#[cfg(target_os = "macos")]
#[must_use]
pub fn system_crash_reports_directory() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;

    // SAFETY: `getpwuid` answers a pointer into the library's own storage for
    // the calling user's record, or null. It is read here and not kept: the one
    // field this function wants is copied into an owned `PathBuf` before the
    // function returns, so nothing outlives the next caller's lookup.
    let home = unsafe {
        let record = libc::getpwuid(libc::getuid());
        if record.is_null() || (*record).pw_dir.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr((*record).pw_dir)
            .to_bytes()
            .to_vec()
    };
    if home.is_empty() {
        return None;
    }
    let home = PathBuf::from(std::ffi::OsStr::from_bytes(&home));
    Some(home.join("Library").join("Logs").join("DiagnosticReports"))
}

/// Nothing to look in. See the macOS arm for why that is an answer and not a
/// gap.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn system_crash_reports_directory() -> Option<PathBuf> {
    None
}

/// Suspend `thread_id`, read where it is, resume it, and say what was there.
///
/// **The thread is suspended for the two kernel calls and nothing else.** See
/// the module comment for why that is not a stylistic preference.
///
/// Never call this on the calling thread: a thread that suspends itself is a
/// thread that will not resume itself, so that case is refused rather than
/// attempted.
#[cfg(all(windows, target_arch = "x86_64"))]
#[must_use]
pub fn capture_thread_stack(thread_id: u32, max_frames: usize) -> StackSample {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::Debug::{
        CONTEXT, CONTEXT_CONTROL_AMD64, GetThreadContext,
    };
    use windows::Win32::System::Threading::{
        OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION,
        THREAD_SUSPEND_RESUME,
    };

    /// `CONTEXT` must be 16-byte aligned for `GetThreadContext` on x86-64 — it
    /// carries `XMM` state — and the `windows` crate declares it `#[repr(C)]`
    /// with a natural alignment of 8. This is the alignment the call needs,
    /// stated where the compiler can honour it rather than hoped for.
    #[repr(C, align(16))]
    struct AlignedContext(CONTEXT);

    if thread_id == current_thread_id() {
        return StackSample::refused("a thread cannot sample itself");
    }

    // ---- everything that can allocate or take a user-mode lock, first ----
    let modules = module_map();
    let mut buffer = vec![0_u8; STACK_SCAN_BYTES];
    let mut context = AlignedContext(CONTEXT::default());
    context.0.ContextFlags = CONTEXT_CONTROL_AMD64;

    // `OpenThread` for three rights and no more: suspend/resume, read the
    // register file, and query — no `THREAD_SET_CONTEXT`, because this reports
    // and never intervenes.
    let handle: HANDLE = match unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
            false,
            thread_id,
        )
    } {
        Ok(handle) => handle,
        Err(_) => return StackSample::refused("OpenThread was refused"),
    };

    // ---- the suspended window: two kernel calls, no allocation, no locks ----
    // `SuspendThread` answers the previous suspend count, or `u32::MAX` on
    // failure. A thread we did not manage to suspend is one we must not resume,
    // because resuming it would decrement a count we never incremented.
    let suspended = unsafe { SuspendThread(handle) } != u32::MAX;
    let mut got_context = false;
    let mut scanned = 0_usize;
    if suspended {
        got_context = unsafe { GetThreadContext(handle, &raw mut context.0) }.is_ok();
        if got_context {
            scanned = read_stack(&mut buffer, context.0.Rsp);
        }
        unsafe { ResumeThread(handle) };
    }
    // ---- resumed; from here on the sample is just bytes ----

    // The window thread is running again whatever else went wrong, so the
    // handle is closed after the resume and its failure is not worth a branch:
    // a process that cannot close a handle it just opened has larger problems
    // than this report.
    let _ = unsafe { CloseHandle(handle) };

    if !suspended {
        return StackSample {
            modules: modules.len(),
            note: Some("SuspendThread was refused"),
            ..StackSample::default()
        };
    }
    if !got_context {
        return StackSample {
            modules: modules.len(),
            note: Some("GetThreadContext was refused"),
            ..StackSample::default()
        };
    }

    let rip = context.0.Rip;
    let rsp = context.0.Rsp;
    StackSample {
        rip,
        rsp,
        rip_site: resolve(&modules, rip, 0),
        frames: scan_frames(&modules, &buffer[..scanned], max_frames),
        scanned_bytes: scanned,
        modules: modules.len(),
        note: (scanned == 0).then_some("the stack could not be read"),
    }
}

#[cfg(all(windows, not(target_arch = "x86_64")))]
#[must_use]
pub fn capture_thread_stack(_thread_id: u32, _max_frames: usize) -> StackSample {
    StackSample::refused("stack capture is written for x86-64 only")
}

#[cfg(not(windows))]
#[must_use]
pub fn capture_thread_stack(_thread_id: u32, _max_frames: usize) -> StackSample {
    StackSample::refused("stack capture is a Windows facility")
}

/// A loaded module and the addresses that belong to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleRange {
    pub name: String,
    pub base: u64,
    pub size: u64,
}

/// Every module in this process, sorted by base address.
///
/// **Called before the suspend**, because it takes the loader lock. See the
/// module comment.
#[cfg(windows)]
#[must_use]
pub fn module_map() -> Vec<ModuleRange> {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::ProcessStatus::{
        EnumProcessModulesEx, GetModuleBaseNameW, GetModuleInformation, LIST_MODULES_ALL,
        MODULEINFO,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    // A pseudo-handle meaning "me": a constant, nothing to close.
    let process = unsafe { GetCurrentProcess() };
    let mut handles = vec![HMODULE::default(); 1024];
    let mut needed = 0_u32;
    let capacity = u32::try_from(size_of::<HMODULE>() * handles.len()).unwrap_or(u32::MAX);
    if unsafe {
        EnumProcessModulesEx(
            process,
            handles.as_mut_ptr(),
            capacity,
            &raw mut needed,
            LIST_MODULES_ALL,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    let count = (needed as usize / size_of::<HMODULE>()).min(handles.len());
    let mut ranges = Vec::with_capacity(count);
    for handle in handles.into_iter().take(count) {
        let mut information = MODULEINFO::default();
        let size = u32::try_from(size_of::<MODULEINFO>()).unwrap_or(u32::MAX);
        if unsafe { GetModuleInformation(process, handle, &raw mut information, size) }.is_err() {
            continue;
        }
        let mut name = [0_u16; 260];
        let written = unsafe { GetModuleBaseNameW(process, Some(handle), &mut name) } as usize;
        let name = if written == 0 {
            format!("0x{:x}", information.lpBaseOfDll as usize)
        } else {
            String::from_utf16_lossy(&name[..written.min(name.len())])
        };
        ranges.push(ModuleRange {
            name,
            base: information.lpBaseOfDll as u64,
            size: u64::from(information.SizeOfImage),
        });
    }
    ranges.sort_by_key(|range| range.base);
    ranges
}

#[cfg(not(windows))]
#[must_use]
pub fn module_map() -> Vec<ModuleRange> {
    Vec::new()
}

/// Read as much of the stack at `rsp` as the kernel will give.
///
/// `ReadProcessMemory` on this process rather than a raw dereference, because a
/// thread stack ends in a guard page and a dereference that walks into it is an
/// access violation in a diagnostic, which is the one thing a diagnostic must
/// never be. It also fails **as a whole** if any page in the span is unmapped,
/// which is why the length halves rather than the call being taken at its word.
#[cfg(all(windows, target_arch = "x86_64"))]
fn read_stack(buffer: &mut [u8], rsp: u64) -> usize {
    use std::ffi::c_void;

    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows::Win32::System::Threading::GetCurrentProcess;

    if rsp == 0 {
        return 0;
    }
    let process = unsafe { GetCurrentProcess() };
    let mut length = buffer.len();
    while length >= 4096 {
        let mut read = 0_usize;
        if unsafe {
            ReadProcessMemory(
                process,
                rsp as *const c_void,
                buffer.as_mut_ptr().cast::<c_void>(),
                length,
                Some(&raw mut read),
            )
        }
        .is_ok()
        {
            return read.min(buffer.len());
        }
        length /= 2;
    }
    0
}

/// Which module `address` belongs to, if any.
///
/// Binary search over the sorted map: the candidate is the last module whose
/// base is at or below the address, and it is a hit only if the address is also
/// inside that module's image.
#[must_use]
pub fn resolve(modules: &[ModuleRange], address: u64, depth: usize) -> Option<ModuleSite> {
    if address < 0x1_0000 {
        return None;
    }
    let index = modules.partition_point(|range| range.base <= address);
    let range = modules.get(index.checked_sub(1)?)?;
    if address >= range.base.checked_add(range.size)? {
        return None;
    }
    Some(ModuleSite {
        address,
        module: range.name.clone(),
        offset: address - range.base,
        depth,
    })
}

/// Every module-resolvable qword in `stack`, innermost first, capped.
///
/// Deliberately unfiltered — see the module comment on why a cleverer filter is
/// a worse diagnostic.
#[must_use]
pub fn scan_frames(modules: &[ModuleRange], stack: &[u8], max_frames: usize) -> Vec<ModuleSite> {
    let mut found = Vec::new();
    if modules.is_empty() {
        return found;
    }
    for (index, chunk) in stack.chunks_exact(8).enumerate() {
        if found.len() >= max_frames {
            break;
        }
        let word = u64::from_le_bytes(chunk.try_into().unwrap_or([0; 8]));
        if let Some(site) = resolve(modules, word, index * 8) {
            found.push(site);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{ModuleRange, ModuleSite, resolve, scan_frames};

    fn map() -> Vec<ModuleRange> {
        vec![
            ModuleRange {
                name: "folio.exe".to_owned(),
                base: 0x1_0000_0000,
                size: 0x0010_0000,
            },
            ModuleRange {
                name: "ntdll.dll".to_owned(),
                base: 0x7fff_0000_0000,
                size: 0x0020_0000,
            },
        ]
    }

    /// An address inside an image is that image's, and an address in the gap
    /// between two images belongs to neither — the case a naive "last base at
    /// or below" lookup gets wrong by naming the module before the gap.
    #[test]
    fn an_address_in_the_gap_between_two_modules_belongs_to_neither() {
        let map = map();
        assert_eq!(
            resolve(&map, 0x1_0000_0040, 0),
            Some(ModuleSite {
                address: 0x1_0000_0040,
                module: "folio.exe".to_owned(),
                offset: 0x40,
                depth: 0,
            })
        );
        assert_eq!(resolve(&map, 0x1_0010_0000, 0), None, "one past the image");
        assert_eq!(resolve(&map, 0x5_0000_0000, 0), None, "the gap");
        assert_eq!(
            resolve(&map, 0x7ffe_ffff_ffff, 0),
            None,
            "just before ntdll"
        );
        assert_eq!(
            resolve(&map, 0x7fff_0000_0000, 0).map(|site| site.module),
            Some("ntdll.dll".to_owned()),
            "the first byte of an image is inside it"
        );
    }

    /// A low word is not an address. Stacks are full of small integers, and
    /// without this floor the first module in the map would swallow every one
    /// of them that happened to be below its size.
    #[test]
    fn a_small_integer_on_the_stack_is_never_read_as_a_return_address() {
        let map = vec![ModuleRange {
            name: "low.dll".to_owned(),
            base: 0,
            size: 0x1000_0000,
        }];
        assert_eq!(resolve(&map, 0, 0), None);
        assert_eq!(resolve(&map, 0xffff, 0), None);
        assert!(resolve(&map, 0x1_0000, 0).is_some());
    }

    /// The scan keeps stack order and its depth is the byte offset from `rsp`,
    /// which is what lets a reader tell an inner frame from an outer one — and
    /// it stops at the cap rather than at the end of the buffer.
    #[test]
    fn the_scan_reports_depth_in_bytes_from_the_stack_pointer_and_honours_the_cap() {
        let map = map();
        let mut stack = Vec::new();
        stack.extend_from_slice(&7_u64.to_le_bytes()); // not an address
        stack.extend_from_slice(&0x1_0000_0100_u64.to_le_bytes());
        stack.extend_from_slice(&0_u64.to_le_bytes());
        stack.extend_from_slice(&0x7fff_0000_0200_u64.to_le_bytes());
        stack.extend_from_slice(&0x1_0000_0300_u64.to_le_bytes());
        let all = scan_frames(&map, &stack, 8);
        assert_eq!(
            all.iter()
                .map(|site| (site.module.as_str(), site.depth))
                .collect::<Vec<_>>(),
            vec![("folio.exe", 8), ("ntdll.dll", 24), ("folio.exe", 32)]
        );
        assert_eq!(scan_frames(&map, &stack, 2).len(), 2, "the cap holds");
    }

    /// An empty module map yields no frames rather than pretending every word
    /// is unresolvable for its own reasons — the report's `modules: 0` is what
    /// tells the reader which of the two happened.
    #[test]
    fn no_module_map_means_no_frames_at_all() {
        let stack = 0x1_0000_0100_u64.to_le_bytes();
        assert!(scan_frames(&[], &stack, 8).is_empty());
    }

    // ── M4-11, read off this file on any machine ───────────────────────────

    /// This file, as text. The macOS arms do not compile on the workstation
    /// where they are written, and the three properties below are decisions
    /// rather than values — which is what a source pin is for.
    const SOURCE: &str = include_str!("hang.rs");

    /// One item of this file, from its signature to the closing brace at the
    /// same indent.
    fn item(needle: &str) -> String {
        let indent: String = needle
            .trim_start_matches('\n')
            .chars()
            .take_while(|character| *character == ' ')
            .collect();
        let closer = format!("\n{indent}}}\n");
        let at = SOURCE
            .find(needle)
            .unwrap_or_else(|| panic!("`{needle}` is in this file"));
        let rest = &SOURCE[at..];
        let end = rest
            .find(&closer)
            .unwrap_or_else(|| panic!("`{needle}` opens an item that is never closed"));
        rest[..end].to_owned()
    }

    /// PIN (M4-11, `docs/DESIGN.md` §13.31) — **the macOS question is put to
    /// Core Foundation, in the common modes, and not to winit.**
    ///
    /// The Windows arm's `WM_NULL` is dispatched by USER32 below anything this
    /// program wrote, which is what makes a thread inside a modal drag answer
    /// it. `CFRunLoopPerformBlock` in `kCFRunLoopCommonModes` is that same
    /// position on this platform; a user event through winit's proxy is not —
    /// it is answered by `bt-app`'s own handler, the very machinery whose
    /// silence raised the suspicion.
    ///
    /// MUTATIONS: swap the mode for `kCFRunLoopDefaultMode` and the first
    /// assertion goes red, and a Folio whose window is being dragged files a
    /// hang report. Drop the `wake_up` and a loop asleep in `mach_msg` answers
    /// whenever the reader next moves the mouse. Answer it through the event
    /// loop proxy instead and the last assertion names the file.
    #[test]
    fn the_macos_liveness_question_is_put_to_the_run_loop_in_the_common_modes() {
        let arm = item("#[cfg(target_os = \"macos\")]\n#[must_use]\npub fn ask_thread_to_answer(");
        assert!(
            arm.contains("MAIN_THREAD_PORT") && arm.contains("Answer::NoWindow"),
            "an id that is not the main thread's owns no window to ask:\n{arm}"
        );
        assert!(
            arm.contains("ask_run_loop_to_answer(&run_loop, timeout)"),
            "and the one that is goes to the main run loop:\n{arm}"
        );
        let body = item("\nfn ask_run_loop_to_answer(");
        assert!(
            body.contains("kCFRunLoopCommonModes"),
            "the question is asked in the mode a tracked drag also turns:\n{body}"
        );
        assert!(
            body.contains("perform_block") && body.contains("run_loop.wake_up()"),
            "the block is queued and the loop is told there is something to do:\n{body}"
        );
        assert!(
            body.contains("recv_timeout(timeout)"),
            "and the wait is bounded by the caller's own timeout:\n{body}"
        );
        // The two arms and not the whole file: this assertion's own text names
        // the things it refuses, and a file that searched itself would find
        // them here.
        let arms = format!("{arm}{body}");
        assert!(
            !arms.contains("EventLoopProxy") && !arms.contains("send_event"),
            "the handshake never goes through winit's user events:\n{arms}"
        );
    }

    /// PIN (M4-11) — **the stack sample stays refused off Windows, and that is
    /// the ticket's decision rather than a gap.**
    ///
    /// `docs/plans/port/backend-inventory-2026-09-12.md` classes
    /// `capture_thread_stack` **X**: the system writes a complete `.ips` for a
    /// process that dies, so a Mach re-implementation of `SuspendThread` plus
    /// `ReadProcessMemory` would be this program writing a worse debugger
    /// against itself.
    ///
    /// MUTATION: give the macOS build an arm that samples and this goes red
    /// naming the arm that was supposed to refuse.
    #[test]
    fn the_stack_sample_is_still_refused_on_every_platform_but_windows() {
        let body = item("#[cfg(not(windows))]\n#[must_use]\npub fn capture_thread_stack(");
        assert!(
            body.contains("StackSample::refused(\"stack capture is a Windows facility\")"),
            "the off-Windows arm refuses, in one sentence:\n{body}"
        );
        assert_eq!(
            SOURCE.matches("\npub fn capture_thread_stack(").count(),
            3,
            "three arms and no fourth: x86-64 Windows, other Windows, everything else"
        );
        assert!(
            !SOURCE.contains(
                "#[cfg(target_os = \"macos\")]\n#[must_use]\npub fn capture_thread_stack"
            ),
            "and none of the three is a macOS sampler"
        );
    }

    /// PIN (M4-11) — **the crash reports are looked for in the account's home
    /// and not in `$HOME`.**
    ///
    /// The `.ips` is written by `ReportCrash`, a system service that knows the
    /// account; this process's `HOME` is whatever it was handed, and every
    /// probe in this workspace hands it a different one. A lookup off the
    /// environment would find nothing on exactly the runs that are measured.
    ///
    /// MUTATION: read `HOME` instead and this goes red; so does the M4 ⑥
    /// acceptance line, which launches from an isolated home on purpose.
    #[test]
    fn the_system_crash_reports_are_looked_for_in_the_accounts_own_home() {
        let body = item("\npub fn system_crash_reports_directory() -> Option<PathBuf> {\n    use");
        assert!(
            body.contains("libc::getpwuid(libc::getuid())"),
            "the home comes from the account record:\n{body}"
        );
        assert!(
            !body.contains("\"HOME\"") && !body.contains("var_os"),
            "and not from the environment this process was handed:\n{body}"
        );
        assert!(
            body.contains("\"Library\"")
                && body.contains("\"Logs\"")
                && body.contains("\"DiagnosticReports\""),
            "under the directory the system files them in:\n{body}"
        );
    }
}

/// **The handshake, against a run loop this process makes and turns on
/// purpose** (M4-11).
///
/// A module of its own because none of it compiles anywhere else, and because
/// what it needs — a second thread with a live run loop, one that turns it and
/// one that does not — is machinery rather than a fixture.
///
/// **A run loop with no input source is not a run loop that waits.**
/// `CFRunLoopRun` returns immediately when there is nothing attached to it, so
/// the turning thread is given one source whose only property is existing. That
/// is not a test convenience: it is the state the real main thread is in, where
/// AppKit's own event source is what keeps the loop asleep instead of finished.
#[cfg(all(test, target_os = "macos"))]
mod macos_handshake_tests {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use objc2_core_foundation::{
        CFRetained, CFRunLoop, CFRunLoopSource, CFRunLoopSourceContext, CFType,
        kCFRunLoopCommonModes, kCFRunLoopDefaultMode,
    };

    use super::{
        Answer, MAIN_THREAD_PORT, ask_run_loop_to_answer, ask_thread_to_answer, current_thread_id,
    };

    /// The address of a run loop another thread owns, carried as a number
    /// because `CFRunLoop` is deliberately neither `Send` nor `Sync` in these
    /// bindings — which is a statement about references, not about
    /// `CFRunLoopPerformBlock`, the call this whole module is about.
    ///
    /// # Safety
    ///
    /// The owning thread holds its `CFRetained` for longer than every caller of
    /// this, so the loop is alive for the whole of the borrow.
    unsafe fn borrow(address: usize) -> &'static CFRunLoop {
        // SAFETY: the caller's contract, above.
        unsafe { &*(address as *const CFRunLoop) }
    }

    /// A source that does nothing and is never signalled. See the module note
    /// for why a loop needs one at all.
    fn something_to_wait_on() -> CFRetained<CFRunLoopSource> {
        unsafe extern "C-unwind" fn perform(_info: *mut c_void) {}

        let mut context = CFRunLoopSourceContext {
            version: 0,
            info: std::ptr::null_mut(),
            retain: None,
            release: None,
            copyDescription: None,
            equal: None,
            hash: None,
            schedule: None,
            cancel: None,
            perform: Some(perform),
        };
        // SAFETY: the context is valid for the whole of the call and its `info`
        // is null, which the callback above never reads.
        unsafe { CFRunLoopSource::new(None, 0, &raw mut context) }
            .expect("a version-0 source is created from a context")
    }

    /// A thread sitting in `CFRunLoopRun`, and the address of the loop it is
    /// sitting in.
    fn a_thread_whose_run_loop_turns() -> (usize, JoinHandle<()>) {
        let (tell, hear) = mpsc::channel::<usize>();
        let handle = std::thread::spawn(move || {
            let run_loop = CFRunLoop::current().expect("every thread has a run loop");
            let source = something_to_wait_on();
            // SAFETY: a Core Foundation constant, initialised before any run
            // loop exists.
            let mode = unsafe { kCFRunLoopDefaultMode }.expect("the default mode is named");
            run_loop.add_source(Some(&source), Some(mode));
            tell.send(std::ptr::from_ref(&*run_loop) as usize)
                .expect("the test is still waiting");
            CFRunLoop::run();
            run_loop.remove_source(Some(&source), Some(mode));
            source.invalidate();
        });
        (
            hear.recv().expect("the thread published its run loop"),
            handle,
        )
    }

    /// Stop the loop from inside itself, which is the one thread
    /// `CFRunLoopStop` has no race with.
    fn let_it_go(address: usize, handle: JoinHandle<()>) {
        // SAFETY: `borrow`'s contract — the thread is still in its loop.
        let run_loop = unsafe { borrow(address) };
        let block = block2::RcBlock::new(|| {
            if let Some(mine) = CFRunLoop::current() {
                mine.stop();
            }
        });
        // SAFETY: a Core Foundation constant.
        let mode: &CFType = unsafe { kCFRunLoopCommonModes }.expect("the common modes are named");
        // SAFETY: as `ask_run_loop_to_answer` — the one call documented to be
        // makeable on another thread's loop.
        unsafe { run_loop.perform_block(Some(mode), Some(&block)) };
        run_loop.wake_up();
        handle
            .join()
            .expect("the loop stopped and the thread ended");
    }

    /// A thread that has a run loop and never turns it — a wedged main thread,
    /// in the one shape a test can build.
    fn a_thread_whose_run_loop_never_turns() -> (usize, Arc<AtomicBool>, JoinHandle<()>) {
        let (tell, hear) = mpsc::channel::<usize>();
        let finished = Arc::new(AtomicBool::new(false));
        let mine = Arc::clone(&finished);
        let handle = std::thread::spawn(move || {
            let run_loop = CFRunLoop::current().expect("every thread has a run loop");
            tell.send(std::ptr::from_ref(&*run_loop) as usize)
                .expect("the test is still waiting");
            while !mine.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        (
            hear.recv().expect("the thread published its run loop"),
            finished,
            handle,
        )
    }

    /// **A loop that is turning answers, well inside the bound.**
    ///
    /// MUTATION: drop the `wake_up` in `ask_run_loop_to_answer` and this hangs
    /// until the timeout, because a loop asleep in `mach_msg` is not told the
    /// queue has something in it.
    #[test]
    fn a_run_loop_that_is_turning_answers_inside_the_bound() {
        let (address, handle) = a_thread_whose_run_loop_turns();
        // SAFETY: `borrow`'s contract — the thread is still in its loop.
        let answer = ask_run_loop_to_answer(unsafe { borrow(address) }, Duration::from_secs(5));
        assert_eq!(answer, Answer::Answered);
        assert_eq!(
            answer.phrase(),
            "the window answered, so the thread is alive",
            "the three sentences are the Windows arm's, unchanged"
        );
        let_it_go(address, handle);
    }

    /// **A loop that is not turning stays silent, and the asking thread comes
    /// back.**
    ///
    /// The second half is the one that matters: a watchdog that blocked on the
    /// thread it is watching would be the fault it exists to report.
    ///
    /// MUTATION: wait on the channel without a timeout and this test never
    /// ends.
    #[test]
    fn a_run_loop_that_never_turns_is_silent_and_does_not_hold_the_asker() {
        let (address, finished, handle) = a_thread_whose_run_loop_never_turns();
        let began = std::time::Instant::now();
        // SAFETY: `borrow`'s contract — the thread is alive and holding the
        // loop.
        let answer = ask_run_loop_to_answer(unsafe { borrow(address) }, Duration::from_millis(200));
        let waited = began.elapsed();
        finished.store(true, Ordering::Relaxed);
        handle.join().expect("the thread ended");
        assert_eq!(answer, Answer::Silent);
        assert!(
            waited < Duration::from_secs(2),
            "the asker came back on its own clock, after {waited:?}"
        );
    }

    /// **Only the main thread registers itself, and every other thread owns no
    /// window to ask.**
    ///
    /// `libtest` runs every test on a thread it spawned, so this body *is* the
    /// non-main case: the id it reads is a real Mach port, and reading it must
    /// not make this thread the one the watchdog will later ask about.
    ///
    /// MUTATION: drop the `pthread_main_np` guard in `current_thread_id` and
    /// the middle assertion goes red — after which a watchdog handed a worker's
    /// id would report on the main run loop instead.
    #[test]
    fn a_thread_that_is_not_the_main_one_registers_nothing_and_owns_no_window() {
        let before = MAIN_THREAD_PORT.load(Ordering::Relaxed);
        let id = current_thread_id();
        assert_ne!(id, 0, "a Mach port name is never MACH_PORT_NULL");
        assert_eq!(
            id,
            current_thread_id(),
            "and it is the same port on every read"
        );
        assert_eq!(
            MAIN_THREAD_PORT.load(Ordering::Relaxed),
            before,
            "a thread that is not the main one records nothing"
        );
        assert_eq!(
            ask_thread_to_answer(id, Duration::from_millis(50)),
            Answer::NoWindow,
            "and there is no window on it to put the question to"
        );
        assert_eq!(
            ask_thread_to_answer(0, Duration::from_millis(50)),
            Answer::NoWindow,
            "nor on a thread that never named itself"
        );
    }
}
