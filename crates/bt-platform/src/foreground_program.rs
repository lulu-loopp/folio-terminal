//! Worker-only observation of the foreground program below one pane's shell (T-PANE-COLUMNS E8).
//!
//! The door returns one normalized leaf image name or `Unknown`; the process tree never crosses
//! the door. Windows takes one ToolHelp snapshot. macOS walks `proc_listchildpids`. An `ssh` client
//! or WSL bridge is a boundary rather than evidence about the remote/guest process.

use std::{collections::BTreeSet, time::Duration};

use crate::admission::WorkerCtx;

/// The private word the Windows observation door gives Folio's short-lived console helper.
pub const CONSOLE_MEMBERS_FLAG: &str = "--console-members";

/// Both the candidate cadence and the console helper's absolute deadline.
pub const OBSERVATION_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForegroundProgram {
    Known(String),
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProcessEntry {
    pid: u32,
    parent: u32,
    image: String,
    started: u64,
}

struct Observation {
    processes: Vec<ProcessEntry>,
    console_members: Option<BTreeSet<u32>>,
}

/// Observe the deepest local descendant of `shell_pid` on a worker.
///
/// ```compile_fail
/// let _ = bt_platform::foreground_program::foreground_program(42);
/// ```
///
/// The missing `&WorkerCtx` is the authority pin: callers outside a thread-door body cannot mint
/// one, and therefore cannot call this process-table door from the window thread.
#[must_use]
pub fn foreground_program(_worker: &WorkerCtx, shell_pid: u32) -> ForegroundProgram {
    imp::observation(_worker, shell_pid).map_or(ForegroundProgram::Unknown, |observation| {
        let order = if cfg!(target_os = "macos") {
            LeafOrder::Youngest
        } else {
            LeafOrder::DeepestThenYoungest
        };
        choose_foreground(
            &observation.processes,
            shell_pid,
            order,
            observation.console_members.as_ref(),
        )
    })
}

/// The standalone helper body. It prints the attached console's process ids, one per line.
/// Failure is `false` and leaves standard output empty.
#[must_use]
pub fn write_console_members(_worker: &WorkerCtx, shell_pid: u32) -> bool {
    imp::write_console_members(shell_pid)
}

fn canonical_image(image: &str) -> String {
    let leaf = image.rsplit(['/', '\\']).next().unwrap_or(image);
    let lower = leaf.to_ascii_lowercase();
    lower
        .strip_suffix(".exe")
        .unwrap_or(lower.as_str())
        .to_owned()
}

fn crosses_remote_boundary(image: &str) -> bool {
    matches!(image, "ssh" | "wsl" | "wslhost" | "wslservice" | "wslrelay")
}

/// A console host is the machinery that draws a console, never a program running in one: Windows
/// starts one beside every process that is given a console of its own, so it is not an answer and
/// does not make its parent an inner node of the tree.
fn is_console_host(image: &str) -> bool {
    matches!(image, "conhost" | "openconsole")
}

#[derive(Clone, Copy)]
enum LeafOrder {
    DeepestThenYoungest,
    Youngest,
}

fn choose_foreground(
    processes: &[ProcessEntry],
    shell_pid: u32,
    order: LeafOrder,
    console_members: Option<&BTreeSet<u32>>,
) -> ForegroundProgram {
    let mut frontier = vec![(shell_pid, 0usize)];
    let mut best: Option<(usize, u64, u32, Option<String>)> = None;
    while let Some((parent, depth)) = frontier.pop() {
        let children = |parent: u32| {
            processes.iter().filter(move |process| {
                process.parent == parent && !is_console_host(&canonical_image(&process.image))
            })
        };
        for child in children(parent) {
            let image = canonical_image(&child.image);
            let boundary = crosses_remote_boundary(&image);
            let has_children = children(child.pid).next().is_some();
            let is_candidate =
                console_members.map_or(!has_children, |members| members.contains(&child.pid));
            if is_candidate {
                let candidate = (
                    depth + 1,
                    child.started,
                    child.pid,
                    (!boundary).then_some(image.clone()),
                );
                if best.as_ref().is_none_or(|current| {
                    matches!(order, LeafOrder::DeepestThenYoungest) && candidate.0 > current.0
                        || (matches!(order, LeafOrder::Youngest) || candidate.0 == current.0)
                            && (candidate.1, candidate.2) > (current.1, current.2)
                }) {
                    best = Some(candidate);
                }
            }
            if !boundary && (has_children || console_members.is_some()) {
                frontier.push((child.pid, depth + 1));
            }
        }
    }
    best.and_then(|(_, _, _, image)| image)
        .map_or(ForegroundProgram::Unknown, ForegroundProgram::Known)
}

#[cfg(any(test, not(any(windows, target_os = "macos"))))]
fn unsupported_observation() -> Option<Observation> {
    None
}

#[cfg(windows)]
mod imp {
    use super::{CONSOLE_MEMBERS_FLAG, OBSERVATION_INTERVAL, Observation, ProcessEntry};
    use std::{
        collections::BTreeSet,
        io::{Read, Write},
        mem::size_of,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::Stdio,
        time::{Duration, Instant},
    };
    use windows::Win32::{
        Foundation::HANDLE,
        System::Console::{AttachConsole, FreeConsole, GetConsoleProcessList},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };

    const HELPER_POLL: Duration = Duration::from_millis(10);

    pub(super) fn observation(
        worker: &crate::admission::WorkerCtx,
        shell_pid: u32,
    ) -> Option<Observation> {
        Some(Observation {
            processes: snapshot(shell_pid)?,
            console_members: Some(console_members(worker, shell_pid)?),
        })
    }

    fn console_members(
        worker: &crate::admission::WorkerCtx,
        shell_pid: u32,
    ) -> Option<BTreeSet<u32>> {
        let executable = std::env::current_exe().ok()?;
        let mut child = crate::quiet_command(executable)
            .args([CONSOLE_MEMBERS_FLAG, &shell_pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + OBSERVATION_INTERVAL;
        let status = loop {
            match child.try_wait().ok()? {
                Some(status) => break status,
                None if Instant::now() < deadline => {
                    crate::wait::sleep_within(worker, HELPER_POLL);
                }
                None => {
                    // This handle names exactly the helper this call started.
                    let _ = child.kill();
                    return None;
                }
            }
        };
        if !status.success() {
            return None;
        }
        let mut output = String::new();
        child.stdout.take()?.read_to_string(&mut output).ok()?;
        let members = output
            .lines()
            .map(str::parse)
            .collect::<Result<BTreeSet<u32>, _>>()
            .ok()?;
        (!members.is_empty()).then_some(members)
    }

    pub(super) fn write_console_members(shell_pid: u32) -> bool {
        // SAFETY: this helper starts consoleless and attaches only to the pane shell pid its
        // parent supplied. It detaches before returning, so no resident Folio process changes
        // console state.
        if unsafe { AttachConsole(shell_pid) }.is_err() {
            return false;
        }
        let members = (|| {
            let mut pids = vec![0u32; 64];
            loop {
                // SAFETY: `pids` is writable for the slice length supplied by the binding.
                let count = unsafe { GetConsoleProcessList(&mut pids) };
                let count = usize::try_from(count).ok()?;
                if count == 0 {
                    return None;
                }
                if count <= pids.len() {
                    pids.truncate(count);
                    return Some(pids);
                }
                pids.resize(count, 0);
            }
        })();
        // SAFETY: the successful attach above made this helper a member of exactly one console.
        let detached = unsafe { FreeConsole() }.is_ok();
        let Some(members) = members.filter(|_| detached) else {
            return false;
        };
        let text = members
            .into_iter()
            .map(|pid| format!("{pid}\n"))
            .collect::<String>();
        let mut output = std::io::stdout().lock();
        output.write_all(text.as_bytes()).is_ok() && output.flush().is_ok()
    }

    /// One ToolHelp snapshot of the process table, narrowed to the shell's descendants before any
    /// process is opened: a start time is read only for a process on the shell's tree, and a
    /// process that started before the parent its snapshot entry names is not that parent's child
    /// (the parent's pid was reused), so its subtree is not the shell's.
    pub(super) fn snapshot(shell_pid: u32) -> Option<Vec<ProcessEntry>> {
        // SAFETY: the call takes a flag and pid and returns an owned snapshot handle.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
        // SAFETY: the handle was created above and is owned by this scope.
        let owned = unsafe { OwnedHandle::from_raw_handle(snapshot.0) };
        let snapshot = HANDLE(owned.as_raw_handle());
        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).ok()?,
            ..Default::default()
        };
        // SAFETY: the snapshot is live and `entry` is writable with its size initialized.
        let mut found = unsafe { Process32FirstW(snapshot, &raw mut entry) }.is_ok();
        let mut table = Vec::new();
        while found {
            let end = entry
                .szExeFile
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(entry.szExeFile.len());
            table.push((
                entry.th32ProcessID,
                entry.th32ParentProcessID,
                String::from_utf16_lossy(&entry.szExeFile[..end]),
            ));
            // SAFETY: the snapshot and writable entry satisfy the same contract as the first call.
            found = unsafe { Process32NextW(snapshot, &raw mut entry) }.is_ok();
        }
        let shell_started = crate::install_flip::started_of(shell_pid)?;
        let mut processes = Vec::new();
        let mut parents = vec![(shell_pid, shell_started)];
        while let Some((parent, parent_started)) = parents.pop() {
            for (pid, _, image) in table
                .iter()
                .filter(|(pid, entry_parent, _)| *entry_parent == parent && *pid != parent)
            {
                let Some(started) = crate::install_flip::started_of(*pid) else {
                    continue;
                };
                if started < parent_started {
                    continue;
                }
                parents.push((*pid, started));
                processes.push(ProcessEntry {
                    pid: *pid,
                    parent,
                    image: image.clone(),
                    started,
                });
            }
        }
        Some(processes)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{Observation, ProcessEntry};
    use std::ffi::{c_char, c_int, c_void};

    const PROC_PIDTBSDINFO: c_int = 3;
    const MAXCOMLEN: usize = 16;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ProcBsdInfo {
        pbi_flags: u32,
        pbi_status: u32,
        pbi_xstatus: u32,
        pbi_pid: u32,
        pbi_ppid: u32,
        pbi_uid: u32,
        pbi_gid: u32,
        pbi_ruid: u32,
        pbi_rgid: u32,
        pbi_svuid: u32,
        pbi_svgid: u32,
        rfu_1: u32,
        pbi_comm: [c_char; MAXCOMLEN],
        pbi_name: [c_char; MAXCOMLEN * 2],
        pbi_nfiles: u32,
        pbi_pgid: u32,
        pbi_pjobc: u32,
        e_tdev: u32,
        e_tpgid: u32,
        pbi_nice: c_int,
        pbi_start_tvsec: u64,
        pbi_start_tvusec: u64,
    }

    unsafe extern "C" {
        fn proc_listchildpids(pid: c_int, buffer: *mut c_void, buffersize: c_int) -> c_int;
        fn proc_pidinfo(
            pid: c_int,
            flavor: c_int,
            arg: u64,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;
        fn proc_name(pid: c_int, buffer: *mut c_void, buffersize: u32) -> c_int;
    }

    pub(super) fn observation(
        _worker: &crate::admission::WorkerCtx,
        shell_pid: u32,
    ) -> Option<Observation> {
        Some(Observation {
            processes: snapshot(shell_pid)?,
            console_members: None,
        })
    }

    fn snapshot(shell_pid: u32) -> Option<Vec<ProcessEntry>> {
        let mut processes = Vec::new();
        let mut parents = vec![i32::try_from(shell_pid).ok()?];
        while let Some(parent) = parents.pop() {
            // SAFETY: a null buffer asks libproc for the required child-pid count.
            let count = unsafe { proc_listchildpids(parent, std::ptr::null_mut(), 0) };
            if count <= 0 {
                continue;
            }
            let count = usize::try_from(count).ok()?;
            let mut pids = vec![0i32; count];
            let capacity = i32::try_from(pids.len().saturating_mul(size_of::<i32>())).ok()?;
            // SAFETY: `pids` is writable for `capacity` bytes.
            let read = unsafe { proc_listchildpids(parent, pids.as_mut_ptr().cast(), capacity) };
            if read <= 0 {
                continue;
            }
            pids.truncate(usize::try_from(read).ok()?.min(pids.len()));
            parents.extend(pids.iter().copied().filter(|pid| *pid > 0));
            for pid in pids.into_iter().filter(|pid| *pid > 0) {
                let mut info = std::mem::MaybeUninit::<ProcBsdInfo>::zeroed();
                let info_size = i32::try_from(size_of::<ProcBsdInfo>()).ok()?;
                // SAFETY: `info` is writable for the declared structure size.
                let got = unsafe {
                    proc_pidinfo(
                        pid,
                        PROC_PIDTBSDINFO,
                        0,
                        info.as_mut_ptr().cast(),
                        info_size,
                    )
                };
                if got != info_size {
                    continue;
                }
                // SAFETY: libproc filled the complete structure above.
                let info = unsafe { info.assume_init() };
                let mut name = [0u8; 1024];
                // SAFETY: `name` is writable for its supplied size.
                let named = unsafe {
                    proc_name(
                        pid,
                        name.as_mut_ptr().cast(),
                        u32::try_from(name.len()).ok()?,
                    )
                };
                if named <= 0 {
                    continue;
                }
                processes.push(ProcessEntry {
                    pid: u32::try_from(pid).ok()?,
                    parent: u32::try_from(parent).ok()?,
                    image: String::from_utf8_lossy(&name[..usize::try_from(named).ok()?])
                        .into_owned(),
                    started: info
                        .pbi_start_tvsec
                        .saturating_mul(1_000_000)
                        .saturating_add(info.pbi_start_tvusec),
                });
            }
        }
        Some(processes)
    }

    pub(super) fn write_console_members(_shell_pid: u32) -> bool {
        false
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::Observation;

    /// The named unsupported-platform refusal. It fabricates no provenance.
    pub(super) fn observation(
        _worker: &crate::admission::WorkerCtx,
        _shell_pid: u32,
    ) -> Option<Observation> {
        super::unsupported_observation()
    }

    pub(super) fn write_console_members(_shell_pid: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(windows, target_os = "macos"))]
    use std::{
        fs,
        time::{Duration, Instant, SystemTime},
    };

    fn process(pid: u32, parent: u32, image: &str, started: u64) -> ProcessEntry {
        ProcessEntry {
            pid,
            parent,
            image: image.to_owned(),
            started,
        }
    }

    #[test]
    fn foreground_program_walk_returns_the_named_multiplexer_grandchild() {
        let table = [
            process(11, 10, "helper.exe", 20),
            process(12, 11, "TMUX.EXE", 30),
        ];
        assert_eq!(
            choose_foreground(&table, 10, LeafOrder::DeepestThenYoungest, None),
            ForegroundProgram::Known("tmux".to_owned())
        );
    }

    /// RED (69a round 2, E8 door) — **a console host is never the foreground program.** Windows
    /// starts `conhost.exe` beside a process that is given a console of its own; it is younger and
    /// deeper than the program it serves, so without this rule it would win every walk.
    ///
    /// MUTATION: drop the `is_console_host` filter from `choose_foreground`'s children.
    #[test]
    fn a_console_host_is_never_the_foreground_program() {
        let table = [
            process(11, 10, "helper.exe", 20),
            process(12, 11, "tmux.exe", 30),
            process(13, 12, "conhost.exe", 40),
            process(14, 11, "OpenConsole.exe", 50),
        ];
        assert_eq!(
            choose_foreground(&table, 10, LeafOrder::DeepestThenYoungest, None),
            ForegroundProgram::Known("tmux".to_owned())
        );
        assert_eq!(
            choose_foreground(&table, 10, LeafOrder::Youngest, None),
            ForegroundProgram::Known("tmux".to_owned())
        );
    }

    #[test]
    fn the_youngest_leaf_wins_at_the_deepest_level() {
        let table = [
            process(11, 10, "old.exe", 20),
            process(12, 10, "new.exe", 30),
        ];
        assert_eq!(
            choose_foreground(&table, 10, LeafOrder::DeepestThenYoungest, None),
            ForegroundProgram::Known("new".to_owned())
        );
    }

    #[test]
    fn macos_policy_chooses_the_youngest_leaf_even_when_it_is_shallower() {
        let table = [
            process(11, 10, "parent", 10),
            process(12, 11, "deep", 20),
            process(13, 10, "young", 30),
        ];
        assert_eq!(
            choose_foreground(&table, 10, LeafOrder::Youngest, None),
            ForegroundProgram::Known("young".to_owned())
        );
    }

    #[test]
    fn wsl_and_ssh_are_unknown_boundaries() {
        for bridge in ["wsl.exe", "ssh.exe"] {
            assert_eq!(
                choose_foreground(
                    &[process(11, 10, bridge, 20)],
                    10,
                    LeafOrder::DeepestThenYoungest,
                    None,
                ),
                ForegroundProgram::Unknown
            );
        }
        assert_eq!(
            choose_foreground(
                &[process(11, 10, "helper", 10), process(12, 11, "ssh", 20),],
                10,
                LeafOrder::DeepestThenYoungest,
                None,
            ),
            ForegroundProgram::Unknown
        );
    }

    /// RED (69a round 3, E8 console ownership) — **an off-console descendant can never supply
    /// trusted provenance.** `tmux.exe` is younger and deeper, but it is detached from the pane's
    /// console; the older console member `screen.exe` is the answer.
    ///
    /// MUTATION: ignore `console_members` in `choose_foreground` and use the ordinary deepest-leaf
    /// choice.
    #[test]
    fn an_off_console_descendant_never_beats_a_console_member() {
        let table = [
            process(11, 10, "screen.exe", 20),
            process(12, 10, "detached.exe", 30),
            process(13, 12, "tmux.exe", 40),
        ];
        let members = BTreeSet::from([10, 11]);
        assert_eq!(
            choose_foreground(&table, 10, LeafOrder::DeepestThenYoungest, Some(&members),),
            ForegroundProgram::Known("screen".to_owned())
        );
    }

    #[cfg(any(windows, target_os = "macos"))]
    const HELPER_MODE: &str = "FOLIO_FOREGROUND_PROGRAM_HELPER_MODE";
    #[cfg(any(windows, target_os = "macos"))]
    const HELPER_NAMED_EXE: &str = "FOLIO_FOREGROUND_PROGRAM_NAMED_EXE";
    #[cfg(any(windows, target_os = "macos"))]
    const HELPER_READY: &str = "FOLIO_FOREGROUND_PROGRAM_READY";
    #[cfg(any(windows, target_os = "macos"))]
    const HELPER_STOP: &str = "FOLIO_FOREGROUND_PROGRAM_STOP";

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn helper_named_grandchild_waits_for_the_parent() {
        if std::env::var(HELPER_MODE).as_deref() != Ok("grandchild") {
            return;
        }
        let stop = std::path::PathBuf::from(std::env::var_os(HELPER_STOP).expect("stop path"));
        while !stop.exists() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn helper_child_spawns_the_named_grandchild() {
        if std::env::var(HELPER_MODE).as_deref() != Ok("child") {
            return;
        }
        let named = std::path::PathBuf::from(
            std::env::var_os(HELPER_NAMED_EXE).expect("named executable path"),
        );
        let ready = std::path::PathBuf::from(std::env::var_os(HELPER_READY).expect("ready path"));
        let stop = std::env::var_os(HELPER_STOP).expect("stop path");
        let mut grandchild = crate::quiet_command(named)
            .args([
                "--exact",
                "foreground_program::tests::helper_named_grandchild_waits_for_the_parent",
            ])
            .env(HELPER_MODE, "grandchild")
            .env(HELPER_STOP, stop)
            .spawn()
            .expect("spawn named grandchild");
        fs::write(&ready, grandchild.id().to_string()).expect("publish grandchild readiness");
        grandchild.wait().expect("join named grandchild");
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn foreground_program_observes_a_real_named_grandchild() {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let sandbox =
            std::env::temp_dir().join(format!("foreground-program-{}-{nonce}", std::process::id()));
        fs::create_dir(&sandbox).expect("create helper sandbox under target");
        let current = std::env::current_exe().expect("current test executable");
        let named = sandbox.join(if cfg!(windows) { "tmux.exe" } else { "tmux" });
        let ready = sandbox.join("ready");
        let stop = sandbox.join("stop");
        fs::copy(&current, &named).expect("copy the test executable under an allowlisted name");
        let mut child = crate::quiet_command(&current)
            .args([
                "--exact",
                "foreground_program::tests::helper_child_spawns_the_named_grandchild",
            ])
            .env(HELPER_MODE, "child")
            .env(HELPER_NAMED_EXE, &named)
            .env(HELPER_READY, &ready)
            .env(HELPER_STOP, &stop)
            .spawn()
            .expect("spawn direct helper child");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(
                child.try_wait().expect("query helper child").is_none(),
                "the helper child exited before its grandchild was ready"
            );
            assert!(
                Instant::now() < deadline,
                "the helper grandchild did not start"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        let shell_pid = child.id();
        #[cfg(target_os = "macos")]
        let observed = crate::spawn_at_priority(
            "foreground-program-door-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| foreground_program(worker, shell_pid),
        )
        .expect("spawn worker-authority test")
        .join()
        .expect("foreground-program worker completes");
        #[cfg(windows)]
        let observed = {
            // The platform test binary has no `--console-members` main; the shipped helper lives
            // in `bt-app`. Keep this real process-tree pin here and give selection the member set
            // the helper supplies. The app/VM row owns the real helper-process proof.
            let grandchild_pid = fs::read_to_string(&ready)
                .expect("read grandchild pid")
                .parse::<u32>()
                .expect("grandchild pid is decimal");
            let table = imp::snapshot(shell_pid).expect("snapshot helper descendants");
            let members = BTreeSet::from([shell_pid, grandchild_pid]);
            choose_foreground(
                &table,
                shell_pid,
                LeafOrder::DeepestThenYoungest,
                Some(&members),
            )
        };

        fs::write(&stop, []).expect("release helper processes");
        assert!(child.wait().expect("join helper child").success());
        fs::remove_file(&ready).expect("remove ready marker");
        fs::remove_file(&stop).expect("remove stop marker");
        fs::remove_file(&named).expect("remove named helper copy");
        fs::remove_dir(&sandbox).expect("remove empty helper sandbox");
        assert_eq!(observed, ForegroundProgram::Known("tmux".to_owned()));
    }

    #[test]
    fn unsupported_platform_refuses_the_foreground_program_door_by_name() {
        assert!(unsupported_observation().is_none());
    }
}
