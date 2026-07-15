//! Exact byte/timing/resize corpus format and deterministic replay support.

use std::{
    io::{self, Read, Write},
    num::NonZeroU16,
};

use thiserror::Error;

const MAGIC: &[u8; 8] = b"BTCRP001";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Corpus {
    pub initial_cols: u16,
    pub initial_rows: u16,
    pub events: Vec<CorpusEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorpusEvent {
    pub at_micros: u64,
    pub kind: EventKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventKind {
    Output(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Exit { code: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Chunking {
    Recorded,
    Fixed(usize),
    Pattern(Vec<usize>),
}

#[derive(Debug, Error)]
pub enum CorpusError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("not a BetterTerminal corpus")]
    BadMagic,
    #[error("unknown corpus event tag {0}")]
    BadTag(u8),
    #[error("invalid zero-sized replay chunk")]
    ZeroChunk,
    #[error("corpus contains a zero terminal dimension")]
    ZeroDimension,
}

impl Corpus {
    pub fn write_to(&self, mut output: impl Write) -> Result<(), CorpusError> {
        output.write_all(MAGIC)?;
        output.write_all(&self.initial_cols.to_le_bytes())?;
        output.write_all(&self.initial_rows.to_le_bytes())?;
        output.write_all(&(self.events.len() as u64).to_le_bytes())?;
        for event in &self.events {
            output.write_all(&event.at_micros.to_le_bytes())?;
            match &event.kind {
                EventKind::Output(bytes) => {
                    output.write_all(&[1])?;
                    output.write_all(&(bytes.len() as u32).to_le_bytes())?;
                    output.write_all(bytes)?;
                }
                EventKind::Resize { cols, rows } => {
                    output.write_all(&[2])?;
                    output.write_all(&cols.to_le_bytes())?;
                    output.write_all(&rows.to_le_bytes())?;
                }
                EventKind::Exit { code } => {
                    output.write_all(&[3])?;
                    output.write_all(&code.to_le_bytes())?;
                }
            }
        }
        Ok(())
    }

    pub fn read_from(mut input: impl Read) -> Result<Self, CorpusError> {
        let mut magic = [0; 8];
        input.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(CorpusError::BadMagic);
        }
        let initial_cols = read_u16(&mut input)?;
        let initial_rows = read_u16(&mut input)?;
        let event_count = read_u64(&mut input)?;
        let mut events = Vec::with_capacity(event_count as usize);
        for _ in 0..event_count {
            let at_micros = read_u64(&mut input)?;
            let mut tag = [0];
            input.read_exact(&mut tag)?;
            let kind = match tag[0] {
                1 => {
                    let len = read_u32(&mut input)? as usize;
                    let mut bytes = vec![0; len];
                    input.read_exact(&mut bytes)?;
                    EventKind::Output(bytes)
                }
                2 => EventKind::Resize {
                    cols: read_u16(&mut input)?,
                    rows: read_u16(&mut input)?,
                },
                3 => EventKind::Exit {
                    code: read_u32(&mut input)?,
                },
                tag => return Err(CorpusError::BadTag(tag)),
            };
            events.push(CorpusEvent { at_micros, kind });
        }
        Ok(Self {
            initial_cols,
            initial_rows,
            events,
        })
    }

    pub fn replay(
        &self,
        chunking: Chunking,
        mut output: impl FnMut(&[u8]),
        mut resize: impl FnMut(NonZeroU16, NonZeroU16),
    ) -> Result<(), CorpusError> {
        let mut pattern_index = 0;
        for event in &self.events {
            match &event.kind {
                EventKind::Output(bytes) => match &chunking {
                    Chunking::Recorded => output(bytes),
                    Chunking::Fixed(size) => {
                        if *size == 0 {
                            return Err(CorpusError::ZeroChunk);
                        }
                        for chunk in bytes.chunks(*size) {
                            output(chunk);
                        }
                    }
                    Chunking::Pattern(pattern) => {
                        if pattern.is_empty() || pattern.contains(&0) {
                            return Err(CorpusError::ZeroChunk);
                        }
                        let mut cursor = 0;
                        while cursor < bytes.len() {
                            let size = pattern[pattern_index % pattern.len()];
                            pattern_index += 1;
                            let end = (cursor + size).min(bytes.len());
                            output(&bytes[cursor..end]);
                            cursor = end;
                        }
                    }
                },
                EventKind::Resize { cols, rows } => {
                    let cols = NonZeroU16::new(*cols).ok_or(CorpusError::ZeroDimension)?;
                    let rows = NonZeroU16::new(*rows).ok_or(CorpusError::ZeroDimension)?;
                    resize(cols, rows);
                }
                EventKind::Exit { .. } => {}
            }
        }
        Ok(())
    }
}

fn read_u16(input: &mut impl Read) -> io::Result<u16> {
    let mut b = [0; 2];
    input.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}
fn read_u32(input: &mut impl Read) -> io::Result<u32> {
    let mut b = [0; 4];
    input.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn read_u64(input: &mut impl Read) -> io::Result<u64> {
    let mut b = [0; 8];
    input.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Corpus {
        Corpus {
            initial_cols: 80,
            initial_rows: 24,
            events: vec![
                CorpusEvent {
                    at_micros: 2,
                    kind: EventKind::Output(vec![0x1b, b'[', b'3', b'1', b'm']),
                },
                CorpusEvent {
                    at_micros: 4,
                    kind: EventKind::Resize {
                        cols: 120,
                        rows: 40,
                    },
                },
                CorpusEvent {
                    at_micros: 8,
                    kind: EventKind::Output(vec![0xf0, 0x9f, 0x98, 0x80]),
                },
                CorpusEvent {
                    at_micros: 9,
                    kind: EventKind::Exit { code: 0 },
                },
            ],
        }
    }

    #[test]
    fn binary_round_trip_is_byte_exact() {
        let corpus = sample();
        let mut bytes = Vec::new();
        corpus.write_to(&mut bytes).unwrap();
        assert_eq!(Corpus::read_from(bytes.as_slice()).unwrap(), corpus);
    }

    #[test]
    fn arbitrary_chunking_preserves_stream_and_resize_order() {
        let corpus = sample();
        let mut bytes = Vec::new();
        let mut sizes = Vec::new();
        corpus
            .replay(
                Chunking::Pattern(vec![1, 3, 2]),
                |chunk| {
                    sizes.push(chunk.len());
                    bytes.extend_from_slice(chunk)
                },
                |cols, rows| assert_eq!((cols.get(), rows.get()), (120, 40)),
            )
            .unwrap();
        assert_eq!(
            bytes,
            [0x1b, b'[', b'3', b'1', b'm', 0xf0, 0x9f, 0x98, 0x80]
        );
        assert!(sizes.iter().all(|size| *size <= 3));
    }

    #[test]
    fn zero_resize_dimension_is_rejected_at_the_corpus_boundary() {
        let mut corpus = sample();
        corpus.events[1].kind = EventKind::Resize { cols: 0, rows: 40 };
        assert!(matches!(
            corpus.replay(Chunking::Recorded, |_| {}, |_, _| {}),
            Err(CorpusError::ZeroDimension)
        ));
    }
}
