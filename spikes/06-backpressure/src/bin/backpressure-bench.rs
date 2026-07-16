use std::{fs::File, io::BufWriter, path::PathBuf};

use anyhow::{Context, Result};
use bt_spike_backpressure::run_benchmark;

fn main() -> Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../docs/spikes/artifacts/06-backpressure.json"));
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let report = run_benchmark()?;
    serde_json::to_writer_pretty(
        BufWriter::new(
            File::create(&output).with_context(|| format!("create {}", output.display()))?,
        ),
        &report,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
