//! **What the machine's memory manager is doing to this process** — one call,
//! two numbers.
//!
//! # Why this exists
//!
//! `bt_app::hang_watch`'s second instrument writes a line every time the window
//! thread holds control past half a second, naming the stations that spent the
//! milliseconds. What that line could not say is **whose milliseconds they
//! were**: a `flush_wheel` charged two seconds is either this program doing two
//! seconds of work, or this program standing still while the operating system
//! reads its working set back off a disk it was trimmed to. The two demand
//! opposite repairs, and until now the log could not tell them apart — which
//! matters most on exactly the machine where the fault is reported, a developer
//! box carrying more committed memory than it has RAM.
//!
//! A page-fault counter separates them, because faulting is the one part of
//! that second the process does not choose. Faults that climb by tens of
//! thousands across a hold, with the resident size climbing beside them, say the
//! time went to the memory manager; a hold that spends two seconds with the
//! counters flat spent them here.
//!
//! # Not a boundary in the sense [`crate::hang`] is
//!
//! That module makes its calls against a **suspended** thread, and the whole of
//! its header is about the two user-mode locks that makes illegal to touch.
//! This one asks the kernel one question about the calling process while
//! everything is running: no handle to release (the Windows arm's process
//! handle is the pseudo-handle, which is a constant), nothing to free, no
//! lock taken, and nothing that can block. It is here rather than in
//! `windows_impl` for the reason [`crate::instance`] is: one subject, two
//! spellings, and a reader who wants to know what "faults" means on a Mac
//! should find both arms on one screen.
//!
//! # What the two numbers are, exactly, on each platform
//!
//! They are not the same quantity, and a reader comparing a Windows log against
//! a Mac one has to know it:
//!
//! * **Windows** — `PROCESS_MEMORY_COUNTERS.PageFaultCount`, which counts
//!   **every** fault this process has taken, the soft ones included (a page
//!   still in RAM on the standby list, handed back without a disk read). That
//!   is the right counter for the question above: a working set that was
//!   trimmed and is being paged back in climbs this number whether or not the
//!   pages had reached the disk yet.
//! * **macOS** — `ri_pageins` out of `proc_pid_rusage`, which counts only the
//!   faults that **went to disk**. Darwin publishes no process-wide soft-fault
//!   counter beside the resident size, so the Mac's number is the stricter of
//!   the two: it is never noise, and it is silent about a compressed page that
//!   came back without a read.
//!
//! The resident size is the same quantity on both — the bytes of this process
//! that are in physical memory at the moment of the call.

/// **One look at this process's memory**, and the pair has to come from a single
/// call: a fault count read a syscall apart from the resident size it is meant
/// to explain is two facts about two moments.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Footprint {
    /// Page faults this process has taken since it started. Cumulative and
    /// monotonic, so the number worth reading is the difference between two
    /// samples rather than either of them.
    ///
    /// **On Windows the kernel counts this in 32 bits** and this widens it, so
    /// a process that took more than 4.29 billion faults has a counter that went
    /// back round. A difference taken across that wrap reads as zero rather than
    /// as an enormous number — see `bt_app::hang_watch`'s subtraction, which
    /// saturates — and that is the harmless direction for a diagnostic to fail
    /// in.
    pub faults: u64,
    /// How many bytes of this process are in physical memory right now.
    pub working_set_bytes: u64,
}

/// **This process's page faults and resident size**, or `None` where the
/// platform does not publish them.
///
/// One system call and no allocation. `None` is not an error path anybody has to
/// handle: the one caller appends these numbers to a log line when they are
/// there and prints the line without them when they are not, which is also what
/// a build on a platform with no arm below gets.
#[cfg(windows)]
#[must_use]
pub fn footprint() -> Option<Footprint> {
    use windows::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };

    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    // SAFETY: `GetCurrentProcess` answers the process pseudo-handle, which is a
    // constant rather than a reference — it is not opened, not counted and not
    // closed. `GetProcessMemoryInfo` writes `size` bytes into `counters`, which
    // is a `PROCESS_MEMORY_COUNTERS` of exactly that many bytes on this stack;
    // it takes no lock, blocks on nothing, and leaves nothing to release.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, size) }.ok()?;
    Some(Footprint {
        faults: u64::from(counters.PageFaultCount),
        working_set_bytes: u64::try_from(counters.WorkingSetSize).unwrap_or(u64::MAX),
    })
}

/// **The same two numbers out of `proc_pid_rusage`** (see the module header for
/// why the Mac's fault count is the stricter one).
///
/// `RUSAGE_INFO_V2` rather than the current flavour: `ri_pageins` and
/// `ri_resident_size` are both in it, it has been the shape of that struct since
/// 10.9, and asking for a newer flavour buys fields nothing here reads at the
/// cost of a call that can be refused on an older system.
///
/// `getrusage(RUSAGE_SELF)` was the other candidate and answers the wrong
/// question: its `ru_maxrss` is the **peak** resident size, so a line built on
/// it would print a pair of numbers that can only climb and would call them the
/// working set.
#[cfg(target_os = "macos")]
#[must_use]
pub fn footprint() -> Option<Footprint> {
    // SAFETY: `zeroed` is a valid `rusage_info_v2` — every field is an integer
    // or an array of them — and the call below overwrites it. `proc_pid_rusage`
    // takes the buffer as `rusage_info_t *`, which is `void **`, so the cast is
    // the one every caller of this function makes; it writes
    // `size_of::<rusage_info_v2>()` bytes for the flavour asked for, which is
    // the struct passed. It asks about this process and takes no reference.
    let info = unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        let taken = libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V2,
            std::ptr::from_mut(&mut info).cast::<libc::rusage_info_t>(),
        );
        if taken != 0 {
            return None;
        }
        info
    };
    Some(Footprint {
        faults: info.ri_pageins,
        working_set_bytes: info.ri_resident_size,
    })
}

/// **Nothing, on a platform that has neither call** — the third arm, on the
/// footing `http_portable.rs` and `video_portable.rs` stand on (§13.27).
///
/// Linux has both numbers and neither is reachable the way the two arms above
/// reach theirs: `getrusage` answers faults but only the *peak* resident size,
/// and the current one lives in `/proc/self/statm`, which is a file read rather
/// than a call and would be a third spelling of this module's contract. Folio
/// does not ship there, so what that build gets is the hold's stations and no
/// footprint beside them — which is the same line this file's callers print
/// whenever a sample is refused, and therefore not a shape anybody has to write
/// a second time.
#[cfg(all(not(windows), not(target_os = "macos")))]
#[must_use]
pub fn footprint() -> Option<Footprint> {
    None
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::footprint;

    /// **The call is wired to a kernel that answers it**, which is the whole of
    /// what this module can be tested for from inside the process it measures.
    ///
    /// Three claims, and each fails on a different mistake: a sample that comes
    /// back `None` is a call refused or a struct sized wrong; a resident size of
    /// zero is a field read at the wrong offset (a live process has pages in
    /// memory); and a fault count that went *down* between two samples is a
    /// number that is not the cumulative counter this module says it is.
    #[test]
    fn a_footprint_is_two_live_numbers_and_the_faults_only_climb() {
        let Some(first) = footprint() else {
            panic!("this platform has an arm above and the kernel answered it")
        };
        assert!(
            first.working_set_bytes > 0,
            "a running process has pages in memory: {first:?}"
        );
        // Enough allocation and touching to make faults plausible, though the
        // assertion below does not need them to have happened — it is the
        // direction that is being pinned, not the amount.
        let mut touched = vec![0_u8; 8 * 1024 * 1024];
        for page in touched.chunks_mut(4096) {
            page[0] = 1;
        }
        let Some(second) = footprint() else {
            panic!("the second call answers as the first one did")
        };
        assert!(
            second.faults >= first.faults,
            "the counter is cumulative, so it cannot go backwards: \
             {first:?} then {second:?}"
        );
        assert!(
            second.working_set_bytes > 0,
            "still resident: {second:?}, and {} bytes were touched",
            touched.len()
        );
    }
}
