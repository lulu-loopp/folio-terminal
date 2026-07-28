use std::{fmt, io::Cursor, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Limits, codecs::png::PngDecoder};

/// Maximum decoded file payload accepted from OSC 1337. The streaming adapter applies the
/// corresponding encoded bound before a worker task is allocated.
pub const MAX_INLINE_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_INLINE_IMAGE_BASE64_BYTES: usize = MAX_INLINE_IMAGE_BYTES.div_ceil(3) * 4;
/// Keep a compressed image from expanding into an unbounded CPU/GPU artifact.
pub const MAX_INLINE_IMAGE_RGBA_BYTES: u64 = 64 * 1024 * 1024;
const MAX_OSC_1337_FILE_HEADER_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineImageTask {
    pub occurrence_id: u64,
    pub encoded: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedInlineImage {
    pub occurrence_id: u64,
    /// Stable content identity. The `image:` prefix keeps it disjoint from math render keys in the
    /// shared GPU texture LRU.
    pub key: String,
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
    /// GIF and APNG are deliberately decoded as one static frame.
    pub animated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InlineImageDecodeError {
    InvalidBase64,
    TooLarge,
    UnsupportedFormat,
    Decode(String),
    InvalidDimensions,
}

impl fmt::Display for InlineImageDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBase64 => formatter.write_str("invalid base64 image payload"),
            Self::TooLarge => formatter.write_str("inline image exceeds its decode limit"),
            Self::UnsupportedFormat => {
                formatter.write_str("inline image format is not supported in v1")
            }
            Self::Decode(error) => write!(formatter, "inline image decode failed: {error}"),
            Self::InvalidDimensions => {
                formatter.write_str("inline image dimensions are invalid or too large")
            }
        }
    }
}

impl std::error::Error for InlineImageDecodeError {}

pub fn decode_inline_image(
    task: InlineImageTask,
) -> Result<DecodedInlineImage, InlineImageDecodeError> {
    let decoded_len =
        decoded_len_upper_bound(&task.encoded).ok_or(InlineImageDecodeError::InvalidBase64)?;
    if decoded_len > MAX_INLINE_IMAGE_BYTES {
        return Err(InlineImageDecodeError::TooLarge);
    }

    let mut bytes = vec![0_u8; decoded_len];
    let written = STANDARD
        .decode_slice(&task.encoded, &mut bytes)
        .map_err(|_| InlineImageDecodeError::InvalidBase64)?;
    bytes.truncate(written);
    if bytes.len() > MAX_INLINE_IMAGE_BYTES {
        return Err(InlineImageDecodeError::TooLarge);
    }

    let mut reader = ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|error| InlineImageDecodeError::Decode(error.to_string()))?;
    let format = reader
        .format()
        .ok_or(InlineImageDecodeError::UnsupportedFormat)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif
    ) {
        return Err(InlineImageDecodeError::UnsupportedFormat);
    }
    let animated = match format {
        ImageFormat::Gif => true,
        ImageFormat::Png => PngDecoder::new(Cursor::new(bytes.as_slice()))
            .and_then(|decoder| decoder.is_apng())
            .map_err(|error| InlineImageDecodeError::Decode(error.to_string()))?,
        _ => false,
    };

    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_INLINE_IMAGE_RGBA_BYTES);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| InlineImageDecodeError::Decode(error.to_string()))?;
    let rgba = image.into_rgba8();
    let (width_px, height_px) = rgba.dimensions();
    let expected = u64::from(width_px)
        .checked_mul(u64::from(height_px))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(InlineImageDecodeError::InvalidDimensions)?;
    if width_px == 0
        || height_px == 0
        || expected > MAX_INLINE_IMAGE_RGBA_BYTES
        || expected != rgba.len() as u64
    {
        return Err(InlineImageDecodeError::InvalidDimensions);
    }

    Ok(DecodedInlineImage {
        occurrence_id: task.occurrence_id,
        key: format!("image:{:032x}", content_hash_128(&bytes)),
        rgba: Arc::from(rgba.into_raw()),
        width_px,
        height_px,
        animated,
    })
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum InlineImageStreamAction {
    Bytes(Vec<u8>),
    Image(Vec<u8>),
    TooLarge,
}

#[derive(Debug)]
enum StreamState {
    Ground,
    Escape,
    OscPrefix { matched: usize, held: Vec<u8> },
    OscPass,
    InlineFile(InlineFileCapture),
    AfterInlineEscape,
}

#[derive(Debug, Default)]
struct InlineFileCapture {
    header: Vec<u8>,
    inline: Option<bool>,
    encoded: Vec<u8>,
    header_too_large: bool,
    payload_too_large: bool,
}

impl InlineFileCapture {
    fn push(&mut self, byte: u8, encoded_limit: usize) {
        if self.inline.is_none() {
            if byte == b':' {
                self.inline = parse_inline_file_header(&self.header);
            } else if self.header.len() < MAX_OSC_1337_FILE_HEADER_BYTES {
                self.header.push(byte);
            } else {
                self.header_too_large = true;
            }
        } else if self.inline == Some(true) && !self.payload_too_large {
            if self.encoded.len() < encoded_limit {
                self.encoded.push(byte);
            } else {
                self.encoded.clear();
                self.payload_too_large = true;
            }
        }
    }

    fn finish(self) -> Option<InlineImageStreamAction> {
        if self.header_too_large || self.inline != Some(true) {
            None
        } else if self.payload_too_large {
            Some(InlineImageStreamAction::TooLarge)
        } else if self.encoded.is_empty() {
            None
        } else {
            Some(InlineImageStreamAction::Image(self.encoded))
        }
    }
}

fn parse_inline_file_header(header: &[u8]) -> Option<bool> {
    let arguments = header.strip_prefix(b"File=")?;
    let mut inline = false;
    for argument in arguments.split(|byte| *byte == b';') {
        if let Some(value) = argument.strip_prefix(b"inline=") {
            inline = value == b"1";
        }
    }
    Some(inline)
}

/// Streaming OSC prefilter at the existing adapter parser seam.
///
/// Unknown OSC 1337 commands are swallowed by `vte::ansi::Performer` before they reach
/// `alacritty_terminal::Term`. Intercepting only the exact `OSC 1337;File=...` prefix here keeps all
/// other terminal bytes on their unchanged path and, unlike a second unbounded `vte::Parser`, can
/// stop retaining an oversized base64 payload before its terminator arrives.
#[derive(Debug)]
pub(crate) struct Osc1337Scanner {
    state: StreamState,
    encoded_limit: usize,
}

impl Default for Osc1337Scanner {
    fn default() -> Self {
        Self {
            state: StreamState::Ground,
            encoded_limit: MAX_INLINE_IMAGE_BASE64_BYTES,
        }
    }
}

impl Osc1337Scanner {
    pub(crate) fn scan(&mut self, bytes: &[u8]) -> Vec<InlineImageStreamAction> {
        const PREFIX: &[u8] = b"1337;";
        let mut actions = Vec::new();
        let mut ordinary = Vec::new();

        for &byte in bytes {
            let state = std::mem::replace(&mut self.state, StreamState::Ground);
            self.state = match state {
                StreamState::Ground => {
                    if byte == 0x1b {
                        StreamState::Escape
                    } else {
                        ordinary.push(byte);
                        StreamState::Ground
                    }
                }
                StreamState::Escape => {
                    if byte == b']' {
                        StreamState::OscPrefix {
                            matched: 0,
                            held: vec![0x1b, b']'],
                        }
                    } else if byte == 0x1b {
                        ordinary.push(0x1b);
                        StreamState::Escape
                    } else {
                        ordinary.extend_from_slice(&[0x1b, byte]);
                        StreamState::Ground
                    }
                }
                StreamState::OscPrefix {
                    mut matched,
                    mut held,
                } => {
                    if PREFIX.get(matched) == Some(&byte) {
                        held.push(byte);
                        matched += 1;
                        if matched == PREFIX.len() {
                            flush_bytes(&mut actions, &mut ordinary);
                            StreamState::InlineFile(InlineFileCapture::default())
                        } else {
                            StreamState::OscPrefix { matched, held }
                        }
                    } else if byte == 0x1b {
                        ordinary.extend_from_slice(&held);
                        StreamState::Escape
                    } else {
                        held.push(byte);
                        ordinary.extend_from_slice(&held);
                        if byte == 0x07 {
                            StreamState::Ground
                        } else {
                            StreamState::OscPass
                        }
                    }
                }
                StreamState::OscPass => {
                    if byte == 0x1b {
                        StreamState::Escape
                    } else {
                        ordinary.push(byte);
                        if byte == 0x07 {
                            StreamState::Ground
                        } else {
                            StreamState::OscPass
                        }
                    }
                }
                StreamState::InlineFile(mut capture) => match byte {
                    0x07 => {
                        finish_capture(&mut actions, &mut ordinary, capture);
                        StreamState::Ground
                    }
                    0x1b => {
                        finish_capture(&mut actions, &mut ordinary, capture);
                        StreamState::AfterInlineEscape
                    }
                    0x18 | 0x1a => {
                        finish_capture(&mut actions, &mut ordinary, capture);
                        ordinary.push(byte);
                        StreamState::Ground
                    }
                    0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f => {
                        StreamState::InlineFile(capture)
                    }
                    _ => {
                        capture.push(byte, self.encoded_limit);
                        StreamState::InlineFile(capture)
                    }
                },
                StreamState::AfterInlineEscape => {
                    if byte == b'\\' {
                        StreamState::Ground
                    } else if byte == b']' {
                        StreamState::OscPrefix {
                            matched: 0,
                            held: vec![0x1b, b']'],
                        }
                    } else if byte == 0x1b {
                        ordinary.push(0x1b);
                        StreamState::Escape
                    } else {
                        ordinary.extend_from_slice(&[0x1b, byte]);
                        StreamState::Ground
                    }
                }
            };
        }
        flush_bytes(&mut actions, &mut ordinary);
        actions
    }
}

fn finish_capture(
    actions: &mut Vec<InlineImageStreamAction>,
    ordinary: &mut Vec<u8>,
    capture: InlineFileCapture,
) {
    flush_bytes(actions, ordinary);
    if let Some(action) = capture.finish() {
        actions.push(action);
    }
}

fn flush_bytes(actions: &mut Vec<InlineImageStreamAction>, ordinary: &mut Vec<u8>) {
    if !ordinary.is_empty() {
        actions.push(InlineImageStreamAction::Bytes(std::mem::take(ordinary)));
    }
}

fn decoded_len_upper_bound(encoded: &[u8]) -> Option<usize> {
    if encoded.is_empty() || !encoded.len().is_multiple_of(4) {
        return None;
    }
    let padding = encoded
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'=')
        .count();
    if padding > 2 {
        return None;
    }
    encoded
        .len()
        .checked_div(4)?
        .checked_mul(3)?
        .checked_sub(padding)
}

/// Stable 128-bit FNV-1a. This is an artifact/cache identity rather than an authenticity boundary;
/// 128 bits makes accidental collision across terminal image payloads negligible without adding a
/// second hashing implementation to the event path (hashing remains worker-only).
fn content_hash_128(bytes: &[u8]) -> u128 {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u128::from(*byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1x1 opaque red PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

    #[test]
    fn png_decodes_to_rgba_artifact_with_content_identity() {
        let decoded = decode_inline_image(InlineImageTask {
            occurrence_id: 7,
            encoded: PNG.as_bytes().to_vec(),
        })
        .unwrap();
        assert_eq!(decoded.occurrence_id, 7);
        assert_eq!((decoded.width_px, decoded.height_px), (1, 1));
        assert_eq!(decoded.rgba.len(), 4);
        assert!(decoded.key.starts_with("image:"));
        assert!(!decoded.animated);
    }

    #[test]
    fn gif_decodes_only_its_first_frame_and_marks_the_record_animated() {
        let decoded = decode_inline_image(InlineImageTask {
            occurrence_id: 8,
            encoded: b"R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==".to_vec(),
        })
        .unwrap();
        assert_eq!((decoded.width_px, decoded.height_px), (1, 1));
        assert_eq!(decoded.rgba.len(), 4);
        assert!(decoded.animated);
    }

    #[test]
    fn apng_decodes_its_default_frame_and_marks_the_record_animated() {
        let mut png = STANDARD.decode(PNG).unwrap();
        let insert_at = 8 + 4 + 4 + 13 + 4;
        let mut animation_chunks = png_chunk(b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]);
        animation_chunks.extend(png_chunk(
            b"fcTL",
            &[
                0, 0, 0, 0, // sequence
                0, 0, 0, 1, // width
                0, 0, 0, 1, // height
                0, 0, 0, 0, // x
                0, 0, 0, 0, // y
                0, 1, // delay numerator
                0, 1, // delay denominator
                0, 0, // dispose, blend
            ],
        ));
        png.splice(insert_at..insert_at, animation_chunks);
        let decoded = decode_inline_image(InlineImageTask {
            occurrence_id: 9,
            encoded: STANDARD.encode(png).into_bytes(),
        })
        .unwrap();
        assert_eq!((decoded.width_px, decoded.height_px), (1, 1));
        assert!(decoded.animated);
    }

    fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::with_capacity(12 + data.len());
        chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
        chunk.extend_from_slice(kind);
        chunk.extend_from_slice(data);
        let mut crc_input = Vec::with_capacity(4 + data.len());
        crc_input.extend_from_slice(kind);
        crc_input.extend_from_slice(data);
        chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        chunk
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = 0_u32.wrapping_sub(crc & 1);
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }

    #[test]
    fn malformed_and_oversized_base64_are_rejected_without_panicking() {
        assert_eq!(
            decode_inline_image(InlineImageTask {
                occurrence_id: 1,
                encoded: b"not base64!".to_vec(),
            }),
            Err(InlineImageDecodeError::InvalidBase64)
        );
        let encoded_len = MAX_INLINE_IMAGE_BYTES.div_ceil(3) * 4 + 4;
        assert_eq!(
            decode_inline_image(InlineImageTask {
                occurrence_id: 2,
                encoded: vec![b'A'; encoded_len],
            }),
            Err(InlineImageDecodeError::TooLarge)
        );
    }

    #[test]
    fn osc_1337_file_reassembles_across_chunks_and_preserves_surrounding_bytes() {
        let mut scanner = Osc1337Scanner::default();
        let mut actions = Vec::new();
        for chunk in [
            b"before\x1b]13".as_slice(),
            b"37;File=name=eA==;in".as_slice(),
            b"line=1:YWJj".as_slice(),
            b"ZA==\x1b\\after".as_slice(),
        ] {
            actions.extend(scanner.scan(chunk));
        }
        assert_eq!(
            actions,
            vec![
                InlineImageStreamAction::Bytes(b"before".to_vec()),
                InlineImageStreamAction::Image(b"YWJjZA==".to_vec()),
                InlineImageStreamAction::Bytes(b"after".to_vec()),
            ]
        );
    }

    #[test]
    fn inline_zero_and_other_osc_are_not_reported_as_images() {
        let mut scanner = Osc1337Scanner::default();
        assert!(scanner.scan(b"\x1b]1337;File=inline=0:YWJj\x07").is_empty());
        assert_eq!(
            scanner.scan(b"\x1b]8;;https://example.test\x1b\\x"),
            vec![InlineImageStreamAction::Bytes(
                b"\x1b]8;;https://example.test\x1b\\x".to_vec()
            )]
        );
    }

    #[test]
    fn encoded_limit_stops_retaining_payload_before_terminator() {
        let mut scanner = Osc1337Scanner {
            state: StreamState::Ground,
            encoded_limit: 8,
        };
        assert_eq!(
            scanner.scan(b"\x1b]1337;File=inline=1:QUJDREVGRw==\x07"),
            vec![InlineImageStreamAction::TooLarge]
        );
    }
}
