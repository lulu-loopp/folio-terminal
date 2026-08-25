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

use std::fmt;

/// How much of the stack is read and scanned for return addresses.
///
/// 128 KiB because a default Windows thread stack reserves 1 MiB and commits
/// far less; a `ReadProcessMemory` that runs off the committed end fails as a
/// whole rather than partially, which is what the halving in [`read_stack`] is
/// for. Deep enough to reach past a WebView2 message pump, small enough that
/// the read is a memcpy and not an event.
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

#[cfg(not(windows))]
#[must_use]
pub fn current_thread_id() -> u32 {
    0
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
}
