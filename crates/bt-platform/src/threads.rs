//! **This process's threads, by the names they were given** (T-IDLE-TURN-COST).
//!
//! One question about the calling process: how many threads it has and what each is called.
//! `bt_app`'s `BT_PERF_TRACE threads` line groups the answer, so a thread count read from outside
//! (the owner's 0.4.7 sat at 328) can be split into the threads this program named and the ones
//! the system and its libraries started in it (graphics drivers, the text services, thread pools),
//! which carry no name.
//!
//! A thread's name is its **description** on Windows (`GetThreadDescription`, which Rust's
//! `thread::Builder::name` sets) and its **pthread name** on macOS (`pthread_setname_np`, read back
//! through `proc_pidinfo`'s per-thread record, which needs no Mach port and so leaves no right to
//! release). A thread with an empty name, or one that ended between the listing and the question,
//! is unnamed. A web page's browser runs in processes of its own and is not in this list.
//!
//! The Windows walk over the system's thread snapshot is also the one the probe's suspension check
//! and its test harness use ([`each_thread_of`]), so there is one copy of it.

/// Each thread of process `pid` in the system's thread snapshot, in snapshot order: `visit` is
/// handed the thread's id, and the walk stops early when it answers `Break`.
#[cfg(windows)]
pub(crate) fn each_thread_of(
    pid: impl Fn(u32) -> bool,
    mut visit: impl FnMut(u32) -> std::ops::ControlFlow<()>,
) -> std::io::Result<()> {
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };

    // SAFETY: a snapshot of the system's threads; its handle moves to `OwnedHandle` at once.
    let snapshot =
        unsafe { OwnedHandle::from_raw_handle(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)?.0) };
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>())
            .expect("thread entry size fits u32"),
        ..THREADENTRY32::default()
    };
    // SAFETY: the snapshot handle is live and `entry` carries its own size.
    let mut more =
        unsafe { Thread32First(HANDLE(snapshot.as_raw_handle()), &raw mut entry) }.is_ok();
    while more {
        if pid(entry.th32OwnerProcessID) && visit(entry.th32ThreadID).is_break() {
            return Ok(());
        }
        // SAFETY: as for `Thread32First`.
        more = unsafe { Thread32Next(HANDLE(snapshot.as_raw_handle()), &raw mut entry) }.is_ok();
    }
    Ok(())
}

/// **Every thread of this process, by name**: one entry per thread, `None` for a thread with no
/// name. In snapshot order.
#[cfg(windows)]
pub fn of_this_process() -> std::io::Result<Vec<Option<String>>> {
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
    use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, GetThreadDescription, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: no arguments; answers this process's id.
    let own = unsafe { GetCurrentProcessId() };
    let mut names = Vec::new();
    each_thread_of(
        |pid| pid == own,
        |thread_id| {
            // SAFETY: opens a thread of this process for a query; the handle moves to
            // `OwnedHandle` at once. A thread that ended meanwhile is unnamed.
            let name = unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, thread_id) }
                .ok()
                .and_then(|thread| {
                    // SAFETY: as above.
                    let thread = unsafe { OwnedHandle::from_raw_handle(thread.0) };
                    // SAFETY: the handle is live; a success hands back a string the system
                    // allocated, which is read and then freed with `LocalFree`, as documented.
                    unsafe {
                        let description =
                            GetThreadDescription(HANDLE(thread.as_raw_handle())).ok()?;
                        let text = description.to_string().ok();
                        LocalFree(Some(HLOCAL(description.0.cast())));
                        text
                    }
                })
                .filter(|name| !name.is_empty());
            names.push(name);
            std::ops::ControlFlow::Continue(())
        },
    )?;
    Ok(names)
}

/// **Every thread of this process, by name** — the macOS arm. See the Windows arm.
#[cfg(target_os = "macos")]
pub fn of_this_process() -> std::io::Result<Vec<Option<String>>> {
    /// `PROC_PIDLISTTHREADS` from `<sys/proc_info.h>`: the process's thread handles, eight bytes
    /// each. The `libc` crate names the per-thread flavour but not this one.
    const PROC_PIDLISTTHREADS: libc::c_int = 6;

    // SAFETY: no arguments; answers this process's id.
    let pid = unsafe { libc::getpid() };
    let mut task = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let task_size = libc::c_int::try_from(std::mem::size_of::<libc::proc_taskinfo>())
        .expect("task information size fits c_int");
    // SAFETY: `task` is writable storage of exactly `task_size` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            task.as_mut_ptr().cast(),
            task_size,
        )
    };
    if written != task_size {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a full record was written.
    let counted = unsafe { task.assume_init() }.pti_threadnum;
    // Room for threads started between the two questions.
    let room = usize::try_from(counted).unwrap_or(0) + 32;
    let mut handles = vec![0_u64; room];
    let handles_size = libc::c_int::try_from(room * std::mem::size_of::<u64>())
        .map_err(|_| std::io::Error::other("the thread list does not fit c_int"))?;
    // SAFETY: `handles` is writable storage of exactly `handles_size` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDLISTTHREADS,
            0,
            handles.as_mut_ptr().cast(),
            handles_size,
        )
    };
    let Ok(written) = usize::try_from(written) else {
        return Err(std::io::Error::last_os_error());
    };
    if written == 0 {
        return Err(std::io::Error::last_os_error());
    }
    handles.truncate(written / std::mem::size_of::<u64>());
    let info_size = libc::c_int::try_from(std::mem::size_of::<libc::proc_threadinfo>())
        .expect("thread information size fits c_int");
    Ok(handles
        .into_iter()
        .map(|handle| {
            let mut info = std::mem::MaybeUninit::<libc::proc_threadinfo>::zeroed();
            // SAFETY: `info` is writable storage of exactly `info_size` bytes; a thread that
            // ended meanwhile answers short and is unnamed.
            let written = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTHREADINFO,
                    handle,
                    info.as_mut_ptr().cast(),
                    info_size,
                )
            };
            if written != info_size {
                return None;
            }
            // SAFETY: a full record was written.
            let info = unsafe { info.assume_init() };
            let bytes: Vec<u8> = info
                .pth_name
                .iter()
                .take_while(|byte| **byte != 0)
                .map(|byte| byte.to_ne_bytes()[0])
                .collect();
            Some(String::from_utf8_lossy(&bytes).into_owned()).filter(|name| !name.is_empty())
        })
        .collect())
}

/// No arm on this platform: the census says it could not be taken.
#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn of_this_process() -> std::io::Result<Vec<Option<String>>> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

#[cfg(test)]
mod tests {
    /// RED (mutation: answer every thread unnamed, or list another process's) — **the threads
    /// this test names are in the census by those names**, as many as it started, and the census
    /// counts at least them and the test's own.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn the_census_names_the_threads_this_process_named() {
        use std::sync::{Arc, Barrier};
        const NAME: &str = "bt-census-probe";
        const STARTED: usize = 3;
        let ready = Arc::new(Barrier::new(STARTED + 1));
        let done = Arc::new(Barrier::new(STARTED + 1));
        let threads: Vec<_> = (0..STARTED)
            .map(|_| {
                let (ready, done) = (Arc::clone(&ready), Arc::clone(&done));
                std::thread::Builder::new()
                    .name(String::from(NAME))
                    .spawn(move || {
                        ready.wait();
                        done.wait();
                    })
                    .expect("spawn a named thread")
            })
            .collect();
        ready.wait();
        let names = super::of_this_process().expect("the census");
        done.wait();
        for thread in threads {
            thread.join().expect("the named thread");
        }
        let probes = names
            .iter()
            .filter(|name| name.as_deref() == Some(NAME))
            .count();
        assert_eq!(probes, STARTED, "{names:?}");
        assert!(names.len() > STARTED, "{names:?}");
    }
}
