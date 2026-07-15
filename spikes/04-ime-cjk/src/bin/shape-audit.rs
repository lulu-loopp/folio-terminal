use std::{fs::File, io::BufWriter, path::PathBuf};

use anyhow::{Context, Result};
use bt_spike_ime_cjk::run_shape_audit;

fn main() -> Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../docs/spikes/artifacts/04-cjk-shaping.json"));
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let report = run_shape_audit()?;
    serde_json::to_writer_pretty(
        BufWriter::new(
            File::create(&output).with_context(|| format!("create {}", output.display()))?,
        ),
        &report,
    )?;
    println!(
        "wrote {} cases to {}; FontSystem::new={}us",
        report.cases.len(),
        output.display(),
        report.font_initialization_micros
    );
    Ok(())
}
