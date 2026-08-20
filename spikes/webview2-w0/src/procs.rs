//! The background ledger: which processes the engine is running, what kind each
//! one is, and what they cost while nobody is looking at them.
//!
//! Two sources, on purpose. WebView2's own `GetProcessInfos` says *what a
//! process is for*; the OS says *what it costs*. Neither can answer the other's
//! question, and gate 8 needs both in one row.

use anyhow::{Context as _, Result};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_KIND, ICoreWebView2Environment, ICoreWebView2Environment8,
};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
    TerminateProcess,
};
use windows::core::Interface as _;

#[derive(Clone, Debug, serde::Serialize)]
pub struct ProcessRow {
    pub pid: u32,
    /// `COREWEBVIEW2_PROCESS_KIND`: 0 browser, 1 renderer, 2 utility, 3 sandbox
    /// helper, 4 GPU, 5 PPAPI plugin, 6 PPAPI broker.
    pub kind: i32,
    pub kind_name: &'static str,
    /// Private bytes — the number that actually goes up when a hidden tab keeps
    /// a page alive.
    pub private_bytes: u64,
    pub working_set: u64,
    /// Kernel + user time since the process started, in milliseconds. Two
    /// readings a known interval apart give the CPU share.
    pub cpu_ms: u64,
}

fn kind_name(kind: i32) -> &'static str {
    match kind {
        0 => "browser",
        1 => "renderer",
        2 => "utility",
        3 => "sandbox-helper",
        4 => "gpu",
        5 => "ppapi-plugin",
        6 => "ppapi-broker",
        _ => "unknown",
    }
}

/// Every process this environment is running, with its cost.
pub fn census(environment: &ICoreWebView2Environment) -> Result<Vec<ProcessRow>> {
    let environment8: ICoreWebView2Environment8 =
        environment.cast().context("ICoreWebView2Environment8")?;
    let collection = unsafe { environment8.GetProcessInfos() }.context("GetProcessInfos")?;
    let mut count = 0u32;
    unsafe { collection.Count(&mut count) }.context("Count")?;
    let mut rows = Vec::with_capacity(count as usize);
    for index in 0..count {
        let info = unsafe { collection.GetValueAtIndex(index) }.context("GetValueAtIndex")?;
        let mut pid = 0i32;
        unsafe { info.ProcessId(&mut pid) }.context("ProcessId")?;
        let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
        unsafe { info.Kind(&mut kind) }.context("Kind")?;
        let (private_bytes, working_set, cpu_ms) = cost(pid as u32);
        rows.push(ProcessRow {
            pid: pid as u32,
            kind: kind.0,
            kind_name: kind_name(kind.0),
            private_bytes,
            working_set,
            cpu_ms,
        });
    }
    Ok(rows)
}

/// Private bytes, working set and accumulated CPU time for one process.
pub fn cost(pid: u32) -> (u64, u64, u64) {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
        else {
            return (0, 0, 0);
        };
        let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
        let memory = GetProcessMemoryInfo(
            handle,
            std::ptr::from_mut(&mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        );
        let (private_bytes, working_set) = if memory.is_ok() {
            (counters.PrivateUsage as u64, counters.WorkingSetSize as u64)
        } else {
            (0, 0)
        };
        let mut creation = Default::default();
        let mut exit = Default::default();
        let mut kernel = Default::default();
        let mut user = Default::default();
        let cpu_ms =
            if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok() {
                (filetime_ms(kernel) + filetime_ms(user)) as u64
            } else {
                0
            };
        let _ = CloseHandle(handle);
        (private_bytes, working_set, cpu_ms)
    }
}

fn filetime_ms(time: windows::Win32::Foundation::FILETIME) -> u128 {
    let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    u128::from(ticks) / 10_000
}

/// Kill one process **this probe started**. Every caller passes a pid that came
/// out of [`census`], which only ever lists the engine this process created.
pub fn terminate(pid: u32) -> Result<()> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid).context("OpenProcess")?;
        let result = TerminateProcess(handle, 0xdead);
        let _ = CloseHandle(handle);
        result.context("TerminateProcess")
    }
}

/// Total private bytes across a census — the one number gate 8 compares before
/// and after hiding a preview.
pub fn total_private_bytes(rows: &[ProcessRow]) -> u64 {
    rows.iter().map(|row| row.private_bytes).sum()
}

pub fn total_cpu_ms(rows: &[ProcessRow]) -> u64 {
    rows.iter().map(|row| row.cpu_ms).sum()
}
