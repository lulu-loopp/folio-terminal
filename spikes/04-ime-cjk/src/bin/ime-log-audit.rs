use std::path::PathBuf;

use anyhow::{Result, bail};
use bt_spike_ime_cjk::audit_log;

fn main() -> Result<()> {
    let mut strict_ime = false;
    let mut path = None;
    for argument in std::env::args_os().skip(1) {
        if argument == "--strict-ime" {
            strict_ime = true;
        } else if path.replace(PathBuf::from(argument)).is_some() {
            bail!("expected one log path");
        }
    }
    let path = path.ok_or_else(|| anyhow::anyhow!("usage: ime-log-audit LOG [--strict-ime]"))?;
    let audit = audit_log(&path, strict_ime)?;
    println!("{}", serde_json::to_string_pretty(&audit)?);
    if !audit.failures.is_empty() {
        bail!("{} audit failure(s)", audit.failures.len());
    }
    Ok(())
}
