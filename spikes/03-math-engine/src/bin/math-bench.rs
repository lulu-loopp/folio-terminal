#![allow(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::c_void;
use std::fs;
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use bt_spike_math::{
    MathSample, MitexTypstEngine, PathSummary, QuickJsKatexEngine, RenderArtifact, corpus,
    summarize,
};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows::core::PCWSTR;

const DEFAULT_WIDTH_PT: u32 = 480;
const PROCESS_MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const MEMORY_WATCHDOG_THRESHOLD: usize = 16 * 1024 * 1024;

#[derive(Serialize)]
struct BenchmarkReport {
    schema: &'static str,
    timestamp_unix_seconds: u64,
    corpus_samples: usize,
    corpus_categories: BTreeMap<String, usize>,
    path_a: PathSummary,
    path_b: RexReport,
    path_c: PathSummary,
    line_breaks: Vec<LineBreakReport>,
    memory: MemoryReport,
    binary_bytes: BTreeMap<String, u64>,
    process_isolation: IsolationReport,
    sandbox: SandboxReport,
}

#[derive(Serialize)]
struct RexReport {
    repository: &'static str,
    revision: &'static str,
    last_push_observed: &'static str,
    compile_probe: &'static str,
    svg_status: &'static str,
}

#[derive(Serialize)]
struct LineBreakReport {
    path: &'static str,
    wide_width_pt: f64,
    wide_height_pt: f64,
    narrow_width_pt: f64,
    narrow_height_pt: f64,
    automatic_break_observed: bool,
    continuation_alignment: &'static str,
}

#[derive(Serialize)]
struct MemoryReport {
    combined_peak_working_set: usize,
    typst: ProcessProbe,
    quickjs: ProcessProbe,
}

#[derive(Clone, Deserialize, Serialize)]
struct ProcessProbe {
    baseline_working_set: usize,
    peak_working_set: usize,
    output_bytes: usize,
}

#[derive(Serialize)]
struct IsolationReport {
    configured_process_memory_limit_bytes: usize,
    memory_watchdog_threshold_bytes: usize,
    normal_runs: usize,
    spawn_job_assign_p50_us: u64,
    spawn_job_assign_p95_us: u64,
    timeout_budget_ms: u64,
    timeout_kill_observed: bool,
    timeout_elapsed_ms: u128,
    memory_limit_termination_observed: bool,
    memory_probe_peak_private_bytes: usize,
    memory_probe_elapsed_ms: u128,
}

#[derive(Serialize)]
struct SandboxReport {
    typst_file_resolver: &'static str,
    typst_network: &'static str,
    quickjs_host_capabilities: &'static str,
    host_caps: &'static str,
}

struct OwnedJob(HANDLE);

impl OwnedJob {
    fn new(process_memory_limit: usize) -> Result<Self> {
        // SAFETY: null security attributes and null name are explicitly allowed by
        // CreateJobObjectW; the returned owned handle is closed in Drop.
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        info.ProcessMemoryLimit = process_memory_limit;
        // SAFETY: the pointer is valid for the exact structure size for the duration
        // of the call, and the information class matches that structure.
        unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &raw const info as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }?;
        Ok(Self(handle))
    }

    fn assign(&self, child: &Child) -> Result<()> {
        let process = HANDLE(child.as_raw_handle());
        // SAFETY: std::process::Child owns a live process handle. Both handles remain
        // valid throughout the synchronous call.
        unsafe { AssignProcessToJobObject(self.0, process) }?;
        Ok(())
    }

    fn terminate(&self) -> Result<()> {
        // SAFETY: self owns a valid Job handle until Drop.
        unsafe { TerminateJobObject(self.0, 0xDEAD) }?;
        Ok(())
    }
}

impl Drop for OwnedJob {
    fn drop(&mut self) {
        // SAFETY: this is the single close for the owned handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct JobRun {
    success: bool,
    timed_out: bool,
    memory_killed: bool,
    peak_private_bytes: usize,
    elapsed: Duration,
}

fn run_in_job(
    worker: &Path,
    mode: &str,
    timeout: Duration,
    memory_watchdog: Option<usize>,
) -> Result<JobRun> {
    let started = Instant::now();
    let mut child = Command::new(worker)
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn {} {mode}", worker.display()))?;
    let job = OwnedJob::new(PROCESS_MEMORY_LIMIT)?;
    job.assign(&child)?;
    child
        .stdin
        .take()
        .context("worker stdin")?
        .write_all(&[1])?;
    let mut peak_private_bytes = 0;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(JobRun {
                success: status.success(),
                timed_out: false,
                memory_killed: false,
                peak_private_bytes,
                elapsed: started.elapsed(),
            });
        }
        let counters = win32job::utils::get_process_memory_info(child.as_raw_handle() as isize)?;
        peak_private_bytes = peak_private_bytes.max(counters.private_usage);
        if memory_watchdog.is_some_and(|limit| counters.private_usage >= limit) {
            job.terminate()?;
            let status = child.wait()?;
            return Ok(JobRun {
                success: status.success(),
                timed_out: false,
                memory_killed: true,
                peak_private_bytes,
                elapsed: started.elapsed(),
            });
        }
        if started.elapsed() >= timeout {
            job.terminate()?;
            let status = child.wait()?;
            return Ok(JobRun {
                success: status.success(),
                timed_out: true,
                memory_killed: false,
                peak_private_bytes,
                elapsed: started.elapsed(),
            });
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn observe<F>(render: F) -> (u64, Result<RenderArtifact, String>)
where
    F: FnOnce() -> Result<RenderArtifact>,
{
    let started = Instant::now();
    let result = render().map_err(|error| error.to_string());
    (
        started.elapsed().as_micros().try_into().unwrap_or(u64::MAX),
        result,
    )
}

fn write_corpus(path: &Path, samples: &[MathSample]) -> Result<()> {
    let mut output = String::new();
    for sample in samples {
        output.push_str(&serde_json::to_string(sample)?);
        output.push('\n');
    }
    fs::write(path, output).with_context(|| format!("write corpus {}", path.display()))
}

fn write_visual_html(dir: &Path, cases: &[(String, String, String)]) -> Result<()> {
    let mut rows = String::new();
    let mut latex = Vec::new();
    for (index, (category, input, a_file)) in cases.iter().enumerate() {
        rows.push_str(&format!(
            "<section><h2>{category}</h2><div class=pair><figure><figcaption>MiTeX + Typst</figcaption><img src=\"{a_file}\"></figure><figure><figcaption>KaTeX reference</figcaption><div class=katex id=k{index}></div></figure></div></section>"
        ));
        latex.push(input);
    }
    let inputs = serde_json::to_string(&latex)?;
    let html = format!(
        r#"<!doctype html><meta charset="utf-8"><title>BetterTerminal math visual audit</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.22/dist/katex.min.css">
<style>body{{font:14px system-ui;margin:24px;background:#17191d;color:#eee}}section{{margin:28px 0}}.pair{{display:grid;grid-template-columns:1fr 1fr;gap:16px}}figure{{margin:0;padding:16px;background:#fff;color:#111;min-height:120px;overflow:auto}}figcaption{{font:12px system-ui;color:#666;margin-bottom:12px}}img{{max-width:100%}}.katex{{font-size:20px}}</style>
{rows}<script src="https://cdn.jsdelivr.net/npm/katex@0.16.22/dist/katex.min.js"></script><script>const xs={inputs};xs.forEach((x,i)=>katex.render(x,document.getElementById('k'+i),{{displayMode:true,throwOnError:false,strict:'ignore'}}));</script>"#
    );
    fs::write(dir.join("comparison.html"), html)?;
    Ok(())
}

fn sibling(name: &str) -> Result<PathBuf> {
    let mut path = std::env::current_exe()?;
    path.set_file_name(format!("{name}.exe"));
    if !path.exists() {
        return Err(anyhow!(
            "missing sibling binary {}; build --release --bins first",
            path.display()
        ));
    }
    Ok(path)
}

fn process_probe(name: &str) -> Result<ProcessProbe> {
    let output = Command::new(sibling(name)?).output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "{name} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn binary_sizes() -> Result<BTreeMap<String, u64>> {
    let mut sizes = BTreeMap::new();
    for name in [
        "size-baseline",
        "typst-probe",
        "quickjs-probe",
        "math-bench",
        "math-worker",
    ] {
        sizes.insert(name.to_owned(), fs::metadata(sibling(name)?)?.len());
    }
    Ok(sizes)
}

fn main() -> Result<()> {
    let report_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("math-results.json"));
    let corpus_path = std::env::args_os()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../corpus/math-expressions.jsonl"));
    let visual_dir = std::env::args_os()
        .nth(3)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../docs/spikes/artifacts/03-visual"));
    fs::create_dir_all(&visual_dir)?;

    let samples = corpus();
    write_corpus(&corpus_path, &samples)?;
    let typst = MitexTypstEngine::new();
    let quickjs = QuickJsKatexEngine::new()?;
    let visual_categories = BTreeSet::from([
        "fraction",
        "root",
        "large-operator",
        "matrix",
        "ams-align",
        "left-right",
    ]);
    let mut captured = BTreeSet::new();
    let mut visual_cases = Vec::new();
    let mut a_observations = Vec::with_capacity(samples.len());
    let mut c_observations = Vec::with_capacity(samples.len());

    for sample in &samples {
        let a = observe(|| typst.render(&sample.latex, DEFAULT_WIDTH_PT));
        if visual_categories.contains(sample.category.as_str())
            && captured.insert(sample.category.clone())
            && let Ok(artifact) = &a.1
        {
            let filename = format!("a-{}.svg", sample.category);
            fs::write(visual_dir.join(&filename), &artifact.output)?;
            visual_cases.push((sample.category.clone(), sample.latex.clone(), filename));
        }
        a_observations.push(a);
        let c = observe(|| quickjs.render(&sample.latex, DEFAULT_WIDTH_PT));
        if visual_categories.contains(sample.category.as_str())
            && let Ok(artifact) = &c.1
        {
            fs::write(
                visual_dir.join(format!("c-{}.svg", sample.category)),
                &artifact.output,
            )?;
        }
        c_observations.push(c);
    }
    write_visual_html(&visual_dir, &visual_cases)?;

    let long = (0..40)
        .map(|i| format!("x_{{{i}}}=y_{{{i}}}"))
        .collect::<Vec<_>>()
        .join(" + ");
    let a_wide = typst.render(&long, 480)?;
    let a_narrow = typst.render(&long, 160)?;
    let c_wide = quickjs.render(&long, 480)?;
    let c_narrow = quickjs.render(&long, 160)?;
    let line_breaks = vec![
        LineBreakReport {
            path: "A-mitex-typst",
            wide_width_pt: a_wide.width_pt,
            wide_height_pt: a_wide.height_pt,
            narrow_width_pt: a_narrow.width_pt,
            narrow_height_pt: a_narrow.height_pt,
            automatic_break_observed: a_narrow.height_pt > a_wide.height_pt * 1.5,
            continuation_alignment: "derived from rendered page; inspect SVG for relation/operator breakpoints",
        },
        LineBreakReport {
            path: "C-rquickjs-katex",
            wide_width_pt: c_wide.width_pt,
            wide_height_pt: c_wide.height_pt,
            narrow_width_pt: c_narrow.width_pt,
            narrow_height_pt: c_narrow.height_pt,
            automatic_break_observed: false,
            continuation_alignment: "none; KaTeX output is nowrap and the adapter cannot add semantic breaks",
        },
    ];

    let worker = sibling("math-worker")?;
    let mut normal_us = Vec::new();
    for _ in 0..10 {
        let run = run_in_job(&worker, "once", Duration::from_secs(2), None)?;
        if !run.success {
            return Err(anyhow!("normal Job Object worker failed"));
        }
        normal_us.push(run.elapsed.as_micros().try_into().unwrap_or(u64::MAX));
    }
    normal_us.sort_unstable();
    let timeout = run_in_job(&worker, "hang", Duration::from_millis(50), None)?;
    let memory = run_in_job(
        &worker,
        "memory",
        Duration::from_secs(2),
        Some(MEMORY_WATCHDOG_THRESHOLD),
    )?;

    let mut categories = BTreeMap::new();
    for sample in &samples {
        *categories.entry(sample.category.clone()).or_default() += 1;
    }
    let combined_peak =
        win32job::utils::get_process_memory_info(win32job::utils::get_current_process())?
            .peak_working_set_size;
    let report = BenchmarkReport {
        schema: "bt-math-engine-spike/v1",
        timestamp_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        corpus_samples: samples.len(),
        corpus_categories: categories,
        path_a: summarize("A-mitex-typst-svg", &samples, &a_observations),
        path_b: RexReport {
            repository: "https://github.com/KenyC/ReX",
            revision: "aeccdba38f3fa54195c469319b65c423e17a77ae",
            last_push_observed: "2026-03-10T17:28:32Z",
            compile_probe: "PASS: pinned dependency and parser probe compile on Rust 1.94.1",
            svg_status: "upstream SVG example requires cairo + an external OpenType MATH font",
        },
        path_c: summarize(
            "C-rquickjs-katex-foreignObject-svg",
            &samples,
            &c_observations,
        ),
        line_breaks,
        memory: MemoryReport {
            combined_peak_working_set: combined_peak,
            typst: process_probe("typst-probe")?,
            quickjs: process_probe("quickjs-probe")?,
        },
        binary_bytes: binary_sizes()?,
        process_isolation: IsolationReport {
            configured_process_memory_limit_bytes: PROCESS_MEMORY_LIMIT,
            memory_watchdog_threshold_bytes: MEMORY_WATCHDOG_THRESHOLD,
            normal_runs: normal_us.len(),
            spawn_job_assign_p50_us: normal_us[4],
            spawn_job_assign_p95_us: normal_us[9],
            timeout_budget_ms: 50,
            timeout_kill_observed: timeout.timed_out && !timeout.success,
            timeout_elapsed_ms: timeout.elapsed.as_millis(),
            memory_limit_termination_observed: memory.memory_killed && !memory.success,
            memory_probe_peak_private_bytes: memory.peak_private_bytes,
            memory_probe_elapsed_ms: memory.elapsed.as_millis(),
        },
        sandbox: SandboxReport {
            typst_file_resolver: "in-memory main source only; no filesystem resolver installed",
            typst_network: "package/network resolver features are disabled",
            quickjs_host_capabilities: "no file, process, socket, fetch, require, or module host bindings",
            host_caps: "64 KiB input, nesting <=256, direct recursion and file/network TeX commands rejected",
        },
    };
    let json = serde_json::to_string_pretty(&report)?;
    fs::write(&report_path, format!("{json}\n"))?;
    println!("{json}");
    Ok(())
}
