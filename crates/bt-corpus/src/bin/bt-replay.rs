use std::{cell::RefCell, env, fs::File, num::NonZeroU32};

use anyhow::{Context, Result};
use bt_corpus::{Chunking, Corpus};
use bt_term::TerminalAdapter;

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .context("usage: bt-replay CORPUS.btcr [CHUNK_SIZE]")?;
    let chunking = env::args()
        .nth(2)
        .map(|value| {
            value
                .parse()
                .map(Chunking::Fixed)
                .context("chunk size must be an integer")
        })
        .transpose()?
        .unwrap_or(Chunking::Recorded);
    let corpus = Corpus::read_from(File::open(path)?)?;
    let columns = NonZeroU32::new(u32::from(corpus.initial_cols))
        .context("corpus initial columns must be non-zero")?;
    let rows = NonZeroU32::new(u32::from(corpus.initial_rows))
        .context("corpus initial rows must be non-zero")?;
    let terminal = RefCell::new(TerminalAdapter::new(columns, rows));
    corpus.replay(
        chunking,
        |bytes| {
            terminal.borrow_mut().feed(bytes);
        },
        |cols, rows| {
            terminal
                .borrow_mut()
                .resize(NonZeroU32::from(cols), NonZeroU32::from(rows));
        },
    )?;
    for line in terminal.borrow().visible_text() {
        println!("{line}");
    }
    Ok(())
}
