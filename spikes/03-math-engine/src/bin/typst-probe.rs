use anyhow::Result;
use bt_spike_math::MitexTypstEngine;
use serde::Serialize;
use win32job::utils::{get_current_process, get_process_memory_info};

#[derive(Serialize)]
struct Probe {
    baseline_working_set: usize,
    peak_working_set: usize,
    output_bytes: usize,
}

fn main() -> Result<()> {
    let baseline = get_process_memory_info(get_current_process())?.working_set_size;
    let engine = MitexTypstEngine::new();
    let artifact = engine.render(r"\frac{1}{2} + \sqrt{x}", 480)?;
    let peak = get_process_memory_info(get_current_process())?.peak_working_set_size;
    println!(
        "{}",
        serde_json::to_string(&Probe {
            baseline_working_set: baseline,
            peak_working_set: peak,
            output_bytes: artifact.output.len(),
        })?
    );
    Ok(())
}
