use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

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
    pub source: InlineImageSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InlineImageSource {
    Osc1337(Vec<u8>),
    LocalPath(PathBuf),
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
    InvalidPath,
    Io(String),
    UnsupportedFormat,
    Decode(String),
    InvalidDimensions,
}

impl fmt::Display for InlineImageDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBase64 => formatter.write_str("invalid base64 image payload"),
            Self::TooLarge => formatter.write_str("inline image exceeds its decode limit"),
            Self::InvalidPath => formatter.write_str("local image path is not admissible"),
            Self::Io(error) => write!(formatter, "local image read failed: {error}"),
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct DecodedImagePayload {
    key: String,
    rgba: Arc<[u8]>,
    width_px: u32,
    height_px: u32,
    animated: bool,
}

/// Stateful decoration-worker decoder. Local paths are keyed lexically after slash/case
/// normalization, so repeated occurrences reuse the one bounded read and decoded pixel artifact.
/// The cache deliberately has no invalidation: file watching belongs to the later M2 buffer model.
#[derive(Debug, Default)]
pub struct InlineImageDecoder {
    local_path_cache: HashMap<String, Result<DecodedImagePayload, InlineImageDecodeError>>,
}

impl InlineImageDecoder {
    pub fn decode(
        &mut self,
        task: InlineImageTask,
    ) -> Result<DecodedInlineImage, InlineImageDecodeError> {
        let payload = match &task.source {
            InlineImageSource::Osc1337(encoded) => decode_osc_payload(encoded)?,
            InlineImageSource::LocalPath(path) => {
                let cache_key = normalized_local_path_key(path);
                if let Some(cached) = self.local_path_cache.get(&cache_key) {
                    cached.clone()?
                } else {
                    let decoded = read_and_decode_local_image(path);
                    self.local_path_cache.insert(cache_key, decoded.clone());
                    decoded?
                }
            }
        };
        Ok(DecodedInlineImage {
            occurrence_id: task.occurrence_id,
            key: payload.key,
            rgba: payload.rgba,
            width_px: payload.width_px,
            height_px: payload.height_px,
            animated: payload.animated,
        })
    }
}

pub fn decode_inline_image(
    task: InlineImageTask,
) -> Result<DecodedInlineImage, InlineImageDecodeError> {
    InlineImageDecoder::default().decode(task)
}

fn decode_osc_payload(encoded: &[u8]) -> Result<DecodedImagePayload, InlineImageDecodeError> {
    let decoded_len =
        decoded_len_upper_bound(encoded).ok_or(InlineImageDecodeError::InvalidBase64)?;
    if decoded_len > MAX_INLINE_IMAGE_BYTES {
        return Err(InlineImageDecodeError::TooLarge);
    }

    let mut bytes = vec![0_u8; decoded_len];
    let written = STANDARD
        .decode_slice(encoded, &mut bytes)
        .map_err(|_| InlineImageDecodeError::InvalidBase64)?;
    bytes.truncate(written);
    if bytes.len() > MAX_INLINE_IMAGE_BYTES {
        return Err(InlineImageDecodeError::TooLarge);
    }
    decode_image_bytes(&bytes)
}

fn read_and_decode_local_image(path: &Path) -> Result<DecodedImagePayload, InlineImageDecodeError> {
    if !is_admissible_local_image_path(path) {
        return Err(InlineImageDecodeError::InvalidPath);
    }
    let mut file =
        File::open(path).map_err(|error| InlineImageDecodeError::Io(error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| InlineImageDecodeError::Io(error.to_string()))?;
    if !metadata.is_file() {
        return Err(InlineImageDecodeError::InvalidPath);
    }
    if metadata.len() > MAX_INLINE_IMAGE_BYTES as u64 {
        return Err(InlineImageDecodeError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(MAX_INLINE_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| InlineImageDecodeError::Io(error.to_string()))?;
    if bytes.len() > MAX_INLINE_IMAGE_BYTES {
        return Err(InlineImageDecodeError::TooLarge);
    }
    decode_image_bytes(&bytes)
}

fn decode_image_bytes(bytes: &[u8]) -> Result<DecodedImagePayload, InlineImageDecodeError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
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
        ImageFormat::Png => PngDecoder::new(Cursor::new(bytes))
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

    Ok(DecodedImagePayload {
        key: format!("image:{:032x}", content_hash_128(bytes)),
        rgba: Arc::from(rgba.into_raw()),
        width_px,
        height_px,
        animated,
    })
}

fn normalized_local_path_key(path: &Path) -> String {
    path.as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn is_admissible_local_image_path(path: &Path) -> bool {
    let text = path.as_os_str().to_string_lossy();
    is_windows_drive_absolute(&text)
        && !text.contains('\0')
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "webp" | "gif"
                )
            })
}

fn is_windows_drive_absolute(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalImagePathCandidate {
    pub path: String,
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Allocation-light lexical candidate scan for the event thread. It recognizes only drive-rooted
/// Windows paths. Unquoted paths stop at whitespace or `]`; quoted paths may contain whitespace
/// and must have a closing quote. Existence, size, content format, and decode remain worker-only.
pub fn detect_local_image_path_candidates(text: &str) -> Vec<LocalImagePathCandidate> {
    let bytes = text.as_bytes();
    let mut candidates = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let quoted = bytes[cursor] == b'"';
        let start = if quoted {
            cursor.saturating_add(1)
        } else {
            cursor
        };
        if !is_drive_prefix_at(bytes, start) || (!quoted && !candidate_start_boundary(bytes, start))
        {
            cursor += 1;
            continue;
        }
        let end = if quoted {
            bytes[start..]
                .iter()
                .position(|byte| *byte == b'"')
                .map(|offset| start + offset)
        } else {
            Some(
                bytes[start..]
                    .iter()
                    .position(|byte| byte.is_ascii_whitespace() || *byte == b']')
                    .map_or(bytes.len(), |offset| start + offset),
            )
        };
        let Some(end) = end.filter(|end| *end > start) else {
            cursor += 1;
            continue;
        };
        let path = &text[start..end];
        if is_admissible_local_image_path(Path::new(path)) {
            candidates.push(LocalImagePathCandidate {
                path: path.to_owned(),
                byte_start: start,
                byte_end: end,
            });
        }
        cursor = if quoted {
            end.saturating_add(1)
        } else {
            end.max(cursor.saturating_add(1))
        };
    }
    candidates
}

fn is_drive_prefix_at(bytes: &[u8], start: usize) -> bool {
    bytes.get(start).is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(start + 1) == Some(&b':')
        && bytes
            .get(start + 2)
            .is_some_and(|byte| matches!(*byte, b'\\' | b'/'))
}

fn candidate_start_boundary(bytes: &[u8], start: usize) -> bool {
    start == 0
        || bytes.get(start.saturating_sub(1)).is_some_and(|byte| {
            byte.is_ascii_whitespace() || matches!(*byte, b'[' | b'(' | b'=' | b':' | b'\'')
        })
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum InlineImageStreamAction {
    Bytes(Vec<u8>),
    Image(Vec<u8>),
    ShellIntegration(ShellIntegrationMarker),
    TooLarge,
}

/// FinalTerm Command Status markers used by PowerShell shell integration. Parameters after the
/// command letter (for example the exit status on `D`) do not affect region ownership in v1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellIntegrationMarker {
    PromptStart,
    CommandStart,
    CommandExecuted,
    CommandFinished,
}

#[derive(Debug)]
enum StreamState {
    Ground,
    Escape,
    OscPrefix { held: Vec<u8> },
    OscPass,
    InlineFile(InlineFileCapture),
    AfterInlineEscape,
    ShellIntegration { payload: Vec<u8>, oversized: bool },
    AfterShellIntegrationEscape { payload: Vec<u8>, oversized: bool },
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
                StreamState::OscPrefix { mut held } => {
                    held.push(byte);
                    let body = &held[2..];
                    if b"1337;".starts_with(body) || b"133;".starts_with(body) {
                        if body == b"1337;" {
                            flush_bytes(&mut actions, &mut ordinary);
                            StreamState::InlineFile(InlineFileCapture::default())
                        } else if body == b"133;" {
                            flush_bytes(&mut actions, &mut ordinary);
                            StreamState::ShellIntegration {
                                payload: Vec::new(),
                                oversized: false,
                            }
                        } else {
                            StreamState::OscPrefix { held }
                        }
                    } else if byte == 0x1b {
                        held.pop();
                        ordinary.extend_from_slice(&held);
                        StreamState::Escape
                    } else {
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
                StreamState::ShellIntegration {
                    mut payload,
                    mut oversized,
                } => match byte {
                    0x07 => {
                        finish_shell_integration(&mut actions, &payload, oversized);
                        StreamState::Ground
                    }
                    0x1b => StreamState::AfterShellIntegrationEscape { payload, oversized },
                    0x18 | 0x1a => StreamState::Ground,
                    0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f => {
                        StreamState::ShellIntegration { payload, oversized }
                    }
                    _ => {
                        if payload.len() < 128 {
                            payload.push(byte);
                        } else {
                            oversized = true;
                        }
                        StreamState::ShellIntegration { payload, oversized }
                    }
                },
                StreamState::AfterShellIntegrationEscape { payload, oversized } => {
                    if byte == b'\\' {
                        finish_shell_integration(&mut actions, &payload, oversized);
                        StreamState::Ground
                    } else if byte == b']' {
                        // A nested OSC terminates the malformed outer marker and begins a fresh
                        // sequence. This bounds recovery without manufacturing a semantic event.
                        StreamState::OscPrefix {
                            held: vec![0x1b, b']'],
                        }
                    } else if byte == 0x1b {
                        StreamState::AfterShellIntegrationEscape { payload, oversized }
                    } else {
                        StreamState::Ground
                    }
                }
            };
        }
        flush_bytes(&mut actions, &mut ordinary);
        actions
    }
}

fn finish_shell_integration(
    actions: &mut Vec<InlineImageStreamAction>,
    payload: &[u8],
    oversized: bool,
) {
    if oversized {
        return;
    }
    let Some((&command, parameters)) = payload.split_first() else {
        return;
    };
    if !parameters.is_empty() && parameters.first() != Some(&b';') {
        return;
    }
    let marker = match command {
        b'A' => ShellIntegrationMarker::PromptStart,
        b'B' => ShellIntegrationMarker::CommandStart,
        b'C' => ShellIntegrationMarker::CommandExecuted,
        b'D' => ShellIntegrationMarker::CommandFinished,
        _ => return,
    };
    actions.push(InlineImageStreamAction::ShellIntegration(marker));
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
    use std::time::{SystemTime, UNIX_EPOCH};

    // 1x1 opaque red PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

    #[test]
    fn png_decodes_to_rgba_artifact_with_content_identity() {
        let decoded = decode_inline_image(InlineImageTask {
            occurrence_id: 7,
            source: InlineImageSource::Osc1337(PNG.as_bytes().to_vec()),
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
            source: InlineImageSource::Osc1337(
                b"R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==".to_vec(),
            ),
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
            source: InlineImageSource::Osc1337(STANDARD.encode(png).into_bytes()),
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
                source: InlineImageSource::Osc1337(b"not base64!".to_vec()),
            }),
            Err(InlineImageDecodeError::InvalidBase64)
        );
        let encoded_len = MAX_INLINE_IMAGE_BYTES.div_ceil(3) * 4 + 4;
        assert_eq!(
            decode_inline_image(InlineImageTask {
                occurrence_id: 2,
                source: InlineImageSource::Osc1337(vec![b'A'; encoded_len]),
            }),
            Err(InlineImageDecodeError::TooLarge)
        );
    }

    #[test]
    fn local_path_candidate_boundaries_are_conservative_and_cc_exact() {
        assert_eq!(
            detect_local_image_path_candidates(r#"[Image: source: C:\Users\weiyi\Pictures\1.png]"#),
            vec![LocalImagePathCandidate {
                path: r"C:\Users\weiyi\Pictures\1.png".to_owned(),
                byte_start: 16,
                byte_end: 45,
            }]
        );
        assert_eq!(
            detect_local_image_path_candidates(
                r#"[Image: source: "C:\Users\weiyi\My Pictures\one two.WEBP"]"#
            )[0]
            .path,
            r"C:\Users\weiyi\My Pictures\one two.WEBP"
        );
        assert_eq!(
            detect_local_image_path_candidates(r"source=C:/tmp/picture.jpeg]ignored.png")[0].path,
            "C:/tmp/picture.jpeg"
        );
        for rejected in [
            r"[Image: source: relative\image.png]",
            r"[Image: source: \\server\share\image.png]",
            r"[Image: source: C:\tmp\image.svg]",
            r#"[Image: source: "C:\tmp\unterminated image.png]"#,
            r"prefixXC:\tmp\image.png",
        ] {
            assert!(
                detect_local_image_path_candidates(rejected).is_empty(),
                "unexpected candidate in {rejected:?}"
            );
        }
    }

    #[test]
    fn local_decoder_reads_once_and_reuses_content_identity_for_each_occurrence() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "betterterminal-inline-path-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("image.png");
        std::fs::write(&path, STANDARD.decode(PNG).unwrap()).unwrap();

        let mut decoder = InlineImageDecoder::default();
        let first = decoder
            .decode(InlineImageTask {
                occurrence_id: 31,
                source: InlineImageSource::LocalPath(path.clone()),
            })
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        let second = decoder
            .decode(InlineImageTask {
                occurrence_id: 32,
                source: InlineImageSource::LocalPath(path),
            })
            .unwrap();
        assert_eq!(first.key, second.key);
        assert_eq!(first.rgba, second.rgba);
        assert_eq!(first.occurrence_id, 31);
        assert_eq!(second.occurrence_id, 32);
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn local_decoder_quietly_rejects_non_images_and_files_over_eight_mib() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "betterterminal-inline-path-reject-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let fake = directory.join("not-an-image.png");
        std::fs::write(&fake, b"plain terminal text").unwrap();
        let oversized = directory.join("oversized.webp");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(MAX_INLINE_IMAGE_BYTES as u64 + 1).unwrap();

        let mut decoder = InlineImageDecoder::default();
        assert_eq!(
            decoder.decode(InlineImageTask {
                occurrence_id: 41,
                source: InlineImageSource::LocalPath(fake.clone()),
            }),
            Err(InlineImageDecodeError::UnsupportedFormat)
        );
        assert_eq!(
            decoder.decode(InlineImageTask {
                occurrence_id: 42,
                source: InlineImageSource::LocalPath(oversized.clone()),
            }),
            Err(InlineImageDecodeError::TooLarge)
        );

        std::fs::remove_file(fake).unwrap();
        std::fs::remove_file(oversized).unwrap();
        std::fs::remove_dir(directory).unwrap();
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
    fn osc_133_markers_reassemble_across_chunks_and_accept_standard_parameters() {
        let mut scanner = Osc1337Scanner::default();
        let mut actions = Vec::new();
        for chunk in [
            b"before\x1b]13".as_slice(),
            b"3;A\x07\x1b]133;B\x1b".as_slice(),
            b"\\command\x1b]133;C\x07output\x1b]133;D;17\x1b\\after".as_slice(),
        ] {
            actions.extend(scanner.scan(chunk));
        }
        assert_eq!(
            actions,
            vec![
                InlineImageStreamAction::Bytes(b"before".to_vec()),
                InlineImageStreamAction::ShellIntegration(ShellIntegrationMarker::PromptStart),
                InlineImageStreamAction::ShellIntegration(ShellIntegrationMarker::CommandStart),
                InlineImageStreamAction::Bytes(b"command".to_vec()),
                InlineImageStreamAction::ShellIntegration(ShellIntegrationMarker::CommandExecuted),
                InlineImageStreamAction::Bytes(b"output".to_vec()),
                InlineImageStreamAction::ShellIntegration(ShellIntegrationMarker::CommandFinished),
                InlineImageStreamAction::Bytes(b"after".to_vec()),
            ]
        );
    }

    #[test]
    fn malformed_and_nested_osc_133_recover_without_manufacturing_markers() {
        let mut scanner = Osc1337Scanner::default();
        assert_eq!(
            scanner.scan(b"x\x1b]133;Bbogus\x07y\x1b]133;B\x1b]133;C\x07z"),
            vec![
                InlineImageStreamAction::Bytes(b"x".to_vec()),
                InlineImageStreamAction::Bytes(b"y".to_vec()),
                InlineImageStreamAction::ShellIntegration(ShellIntegrationMarker::CommandExecuted),
                InlineImageStreamAction::Bytes(b"z".to_vec()),
            ]
        );
    }

    #[test]
    fn osc_133_decisions_are_invariant_at_every_chunk_boundary() {
        let stream = b"pre\x1b]133;A\x07\x1b]133;B\x1b\\cmd\x1b]133;C\x07out\x1b]133;D;0\x1b\\post";
        fn normalized(chunks: &[&[u8]]) -> (Vec<u8>, Vec<ShellIntegrationMarker>) {
            let mut scanner = Osc1337Scanner::default();
            let mut bytes = Vec::new();
            let mut markers = Vec::new();
            for chunk in chunks {
                for action in scanner.scan(chunk) {
                    match action {
                        InlineImageStreamAction::Bytes(part) => bytes.extend(part),
                        InlineImageStreamAction::ShellIntegration(marker) => markers.push(marker),
                        InlineImageStreamAction::Image(_) | InlineImageStreamAction::TooLarge => {
                            panic!("fixture contains no image")
                        }
                    }
                }
            }
            (bytes, markers)
        }
        let whole = normalized(&[stream]);
        for split in 0..=stream.len() {
            assert_eq!(
                normalized(&[&stream[..split], &stream[split..]]),
                whole,
                "split at byte {split}"
            );
        }
        let bytewise = stream.iter().map(std::slice::from_ref).collect::<Vec<_>>();
        assert_eq!(normalized(&bytewise), whole);
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
