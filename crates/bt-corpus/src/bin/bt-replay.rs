use std::{cell::RefCell, env, fs::File};

use anyhow::{Context, Result};
use bt_corpus::{Chunking, Corpus};
use bt_term::TerminalAdapter;

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .context("usage: bt-replay CORPUS.btcr [CHUNK_SIZE]")?;
    let chunking = env::args().nth(2).map_or(Chunking::Recorded, |value| {
        Chunking::Fixed(value.parse().expect("chunk size must be an integer"))
    });
    let corpus = Corpus::read_from(File::open(path)?)?;
    let terminal = RefCell::new(TerminalAdapter::new(
        corpus.initial_cols as usize,
        corpus.initial_rows as usize,
    ));
    corpus.replay(
        chunking,
        |bytes| {
            terminal.borrow_mut().feed(bytes);
        },
        |cols, rows| {
            terminal.borrow_mut().resize(cols as usize, rows as usize);
        },
    )?;
    for line in terminal.borrow().visible_text() {
        println!("{line}");
    }
    Ok(())
}
