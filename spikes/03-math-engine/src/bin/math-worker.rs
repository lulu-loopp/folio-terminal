use std::io::Read;
use std::thread;
use std::time::Duration;

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "once".to_owned());
    let mut signal = [0_u8; 1];
    std::io::stdin().read_exact(&mut signal)?;
    match mode.as_str() {
        "once" => Ok(()),
        "hang" => loop {
            thread::sleep(Duration::from_secs(1));
        },
        "memory" => {
            let mut blocks = Vec::new();
            loop {
                let block = vec![0xA5_u8; 16 * 1024 * 1024];
                std::hint::black_box(&block);
                blocks.push(block);
            }
        }
        _ => bail!("unknown worker mode: {mode}"),
    }
}
