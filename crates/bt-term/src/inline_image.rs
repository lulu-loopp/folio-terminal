use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{
    ImageBuffer, ImageFormat, ImageReader, Limits, Rgba, codecs::png::PngDecoder,
    imageops::FilterType,
};

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

/// Resample a native decode into the exact pixel box the current layout will show it in.
///
/// A band hands the renderer display-resolution pixels, never the native decode: a 3840x2400
/// wallpaper shown 1280px wide is 35 MiB of texture for 4 MiB of visible detail, and two of those
/// evict each other out of the GPU texture budget on every frame. Resampling is worker-only work —
/// see `scale_inline_image`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineImageScaleTask {
    pub occurrence_id: u64,
    /// Identity of the native decode this request resamples. A completion whose content key no
    /// longer matches the record's decode is a stale answer to a superseded question.
    pub content_key: String,
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
    pub display_width_px: u32,
    pub display_height_px: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScaledInlineImage {
    pub occurrence_id: u64,
    pub content_key: String,
    /// `<content key>@<width>x<height>`. The display size is part of the texture identity, so a
    /// zoom or DPI change asks the shared GPU LRU a different question and gets a re-raster
    /// instead of a stale raster stretched by the sampler.
    pub key: String,
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
}

/// The texture identity of one content at one display size.
pub fn display_texture_key(content_key: &str, width_px: u32, height_px: u32) -> String {
    format!("{content_key}@{width_px}x{height_px}")
}

/// Resample a decoded image into its display box with a Lanczos3 kernel.
///
/// Worker-only: a wallpaper-sized downscale costs tens of milliseconds, which is exactly why the
/// event thread hands this out instead of doing it inline. The native decode stays in the
/// decoder's cache, so a later display size is one resample and never a second disk read.
pub fn scale_inline_image(task: &InlineImageScaleTask) -> ScaledInlineImage {
    let key = display_texture_key(
        &task.content_key,
        task.display_width_px,
        task.display_height_px,
    );
    let native = (task.width_px, task.height_px);
    let display = (task.display_width_px, task.display_height_px);
    let rgba = if native == display {
        Arc::clone(&task.rgba)
    } else {
        // `Arc<[u8]>` derefs to the sample slice, so the source buffer is borrowed rather than
        // copied; only the resampled result is allocated.
        let source: ImageBuffer<Rgba<u8>, Arc<[u8]>> =
            ImageBuffer::from_raw(task.width_px, task.height_px, Arc::clone(&task.rgba))
                .expect("scale task carries a decoded RGBA buffer of its stated dimensions");
        Arc::from(
            image::imageops::resize(
                &source,
                task.display_width_px,
                task.display_height_px,
                FilterType::Lanczos3,
            )
            .into_raw(),
        )
    };
    ScaledInlineImage {
        occurrence_id: task.occurrence_id,
        content_key: task.content_key.clone(),
        key,
        rgba,
        width_px: task.display_width_px,
        height_px: task.display_height_px,
    }
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
    let Some(format) = reader.format() else {
        // Not a recognized raster container. SVG is admitted as a static raster (preview matrix
        // §2); the parse itself is the validity check — XML that usvg rejects is simply an
        // unsupported payload, with no sniffing heuristics in front of it.
        return decode_svg_bytes(bytes);
    };
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

fn decode_svg_bytes(bytes: &[u8]) -> Result<DecodedImagePayload, InlineImageDecodeError> {
    let raster = bt_math::rasterize_svg_document(bytes).map_err(|error| match error {
        // Bytes that fail the SVG parse are simply not any admitted format — the same quiet
        // verdict a text file with a .png extension has always received.
        bt_math::SvgRasterError::Parse(_) => InlineImageDecodeError::UnsupportedFormat,
        bt_math::SvgRasterError::Dimensions(_) => InlineImageDecodeError::InvalidDimensions,
    })?;
    Ok(DecodedImagePayload {
        key: format!("image:{:032x}", content_hash_128(bytes)),
        rgba: Arc::from(raster.rgba),
        width_px: raster.width_px,
        height_px: raster.height_px,
        animated: false,
    })
}

fn normalized_local_path_key(path: &Path) -> String {
    path.as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

/// The slash/case-normalized identity every local-image cache layer keys on. Exposed so callers
/// (e.g. the hover-peek cache) share the decoder's notion of "same file" instead of re-deriving it.
pub fn normalized_local_image_path_key(path: &Path) -> String {
    normalized_local_path_key(path)
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
                    "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg"
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
/// Windows paths. Unquoted paths open at a token boundary (`candidate_start_boundary`) and close
/// at whitespace or a closing delimiter (`is_path_terminator_char`); quoted paths may contain
/// whitespace and any delimiter, and must have a closing quote. Existence, size, content format,
/// and decode remain worker-only.
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
        if !is_drive_prefix_at(bytes, start) || (!quoted && !candidate_start_boundary(text, start))
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
            Some(token_end(text, start))
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

/// Characters that could legitimately be the tail of a longer token, so a drive prefix that
/// follows one is a suffix of that token rather than a path of its own: alphanumerics of **any**
/// script (`prefixXC:\a.png`, `图片D:\a.png`) plus the path-structure characters that continue a
/// path or a filename stem (`sub\D:\a.png`, `file:///D:/a.png`, `v1.2D:\a.png`, `x-D:\a.png`).
///
/// `/` and `\` are on this list for one load-bearing reason: it is what keeps the `D:/…` embedded
/// in a `file:///D:/…` URI from being read as a native path. URIs reach the peek through
/// `detect_local_image_uri_candidates`, which decodes them properly; they must never be half-read
/// here.
///
/// Everything else opens a path — whitespace of any width, opening brackets and quotes of any
/// script (`(`、`（`、`「`、`“`), separators (`:`、`：`、`=`、`,`), and the rest of punctuation.
/// That generality is the point: a path is no less a path for sitting in CJK prose.
fn is_path_tail_char(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '/' | '\\' | '.' | '-' | '_')
}

/// Closing delimiters that end an unquoted path token. Unicode General_Category Pe
/// (Close_Punctuation) as it appears in terminal prose, plus ASCII `>` and its full-width form —
/// `<>` is a bracket pair everywhere and neither character is legal in a Windows path anyway.
///
/// Final_Punctuation (Pf) is deliberately split: `»`、`›`、`”` are admitted because they only ever
/// close, while `’` is not, because it doubles as an apostrophe inside ordinary filenames
/// (`Bob’s photo.png`). A filename that really ends in a closing delimiter can still be quoted.
fn is_closing_delimiter(character: char) -> bool {
    matches!(
        character,
        ')' | ']'
            | '}'
            | '>'
            | '\u{ff09}' // ）
            | '\u{ff3d}' // ］
            | '\u{ff5d}' // ｝
            | '\u{ff60}' // ｠
            | '\u{ff63}' // ｣
            | '\u{ff1e}' // ＞
            | '\u{3009}' // 〉
            | '\u{300b}' // 》
            | '\u{300d}' // 」
            | '\u{300f}' // 』
            | '\u{3011}' // 】
            | '\u{3015}' // 〕
            | '\u{3017}' // 〗
            | '\u{3019}' // 〙
            | '\u{301b}' // 〛
            | '\u{27e9}' // ⟩
            | '\u{27eb}' // ⟫
            | '\u{00bb}' // »
            | '\u{203a}' // ›
            | '\u{201d}' // ”
    )
}

fn is_path_terminator_char(character: char) -> bool {
    character.is_whitespace() || is_closing_delimiter(character)
}

/// End of the unquoted token starting at `start` (a byte offset on a character boundary).
fn token_end(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find(|(_, character)| is_path_terminator_char(*character))
        .map_or(text.len(), |(offset, _)| start + offset)
}

/// Whether a candidate at byte offset `start` opens a token rather than continuing one. The
/// decision is per *character*: the byte before a candidate is the last byte of a multi-byte
/// character in every CJK-adjacent line, so a byte test would reject `（D:\a.png）` and
/// `路径：D:\a.png` on the strength of a UTF-8 continuation byte alone.
fn candidate_start_boundary(text: &str, start: usize) -> bool {
    start == 0
        || text[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !is_path_tail_char(character))
}

/// Peek-only lexical scan for `file://` URIs that name an admissible local image.
///
/// Kept apart from `detect_local_image_path_candidates` because the two shapes earn different
/// presentations: a printed native path grows an inline band on the primary screen, a URI never
/// does. A URI is a reference to a file, not the file's name in the flow, so it answers hover and
/// nothing else. `path` on the returned candidate is therefore the **resolved** local path, while
/// the byte span covers the URI text that must be hovered to reach it.
pub fn detect_local_image_uri_candidates(text: &str) -> Vec<LocalImagePathCandidate> {
    const SCHEME: &str = "file://";
    let bytes = text.as_bytes();
    let mut candidates = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        // The scheme is ASCII, so a byte match proves `cursor` is a character boundary before
        // `candidate_start_boundary` decodes the character preceding it.
        let scheme_matches = bytes
            .get(cursor..cursor + SCHEME.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(SCHEME.as_bytes()));
        if !scheme_matches || !candidate_start_boundary(text, cursor) {
            cursor += 1;
            continue;
        }
        let end = uri_token_end(text, cursor + SCHEME.len());
        if let Some(path) = file_uri_to_local_image_path(&text[cursor..end]) {
            candidates.push(LocalImagePathCandidate {
                path: path.to_string_lossy().into_owned(),
                byte_start: cursor,
                byte_end: end,
            });
        }
        cursor = end.max(cursor + 1);
    }
    candidates
}

/// Every image one line of text offers the hover peek: native drive-rooted paths and `file://`
/// URIs. Inline admission scans only the former, so this is the peek's own reading of the line.
/// The two shapes cannot overlap — a URI's embedded `D:/…` is preceded by `/`, which
/// `is_path_tail_char` rejects as a path opening.
pub fn detect_peek_image_candidates(text: &str) -> Vec<LocalImagePathCandidate> {
    let mut candidates = detect_local_image_path_candidates(text);
    candidates.extend(detect_local_image_uri_candidates(text));
    candidates
}

/// End of a URI token that starts at `scheme_end`. A URI is ASCII by construction (RFC 3986 —
/// anything else must be percent-encoded), so any character outside the allowed set ends it,
/// which is what lets a full-width `）` close a URI that ASCII `)` cannot. Trailing sentence
/// punctuation is then released, matching `bt_transcript::detect_http_urls`, whose prose-boundary
/// problem is the same one.
fn uri_token_end(text: &str, scheme_end: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = scheme_end;
    while end < bytes.len() && is_uri_byte(bytes[end]) {
        end += 1;
    }
    while end > scheme_end
        && matches!(
            bytes[end - 1],
            b')' | b'.' | b',' | b';' | b':' | b'!' | b'?'
        )
    {
        end -= 1;
    }
    end
}

/// RFC 3986 unreserved + reserved + the percent sign.
fn is_uri_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'%'
                | b':'
                | b'/'
                | b'?'
                | b'#'
                | b'['
                | b']'
                | b'@'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
        )
}

/// Resolve a `file://` URI to the local image path it names, or `None` when it names something
/// this terminal will not preview.
///
/// The URI is a *reference*; the gates it must pass are the same ones printed path text passes
/// (`is_admissible_local_image_path`: drive-rooted and an allowlisted image extension), so no
/// shape buys a privilege another lacks. Existence, size, and decode stay worker-side as always.
///
/// Resolution is per URI segment: each is percent-decoded on its own and the results are joined
/// with `\`. That is what RFC 3986 means — a `%2F` inside a segment decodes to a literal `/` in a
/// filename, never to a separator — and it costs nothing, since such a name simply fails to exist.
///
/// A non-empty authority other than `localhost` is a remote share (`file://host/share/a.png`) and
/// is rejected: this is the *local* image peek, and a UNC fetch is not a hover's business.
pub fn file_uri_to_local_image_path(uri: &str) -> Option<PathBuf> {
    let rest = uri
        .get(..7)
        .filter(|scheme| scheme.eq_ignore_ascii_case("file://"))
        .map(|scheme| &uri[scheme.len()..])?;
    let (authority, path) = rest.split_at(rest.find('/')?);
    if !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost")) {
        return None;
    }
    // Query and fragment are not part of the path; a filename containing `?` or `#` must have
    // percent-encoded them.
    let path = &path[..path.find(['?', '#']).unwrap_or(path.len())];
    let mut native = String::new();
    for segment in path.split('/').skip(1) {
        let decoded = percent_decode(segment)?;
        if decoded.is_empty() {
            return None;
        }
        if !native.is_empty() {
            native.push('\\');
        }
        native.push_str(&decoded);
    }
    let path = PathBuf::from(native);
    is_admissible_local_image_path(&path).then_some(path)
}

/// Percent-decode one URI segment. `None` when an escape is malformed, the result is not UTF-8,
/// or it carries a control character — each of which means the text was never a path we may read.
fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = segment
                .get(index + 1..index + 3)
                .filter(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()))?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).ok()?;
    (!decoded.chars().any(char::is_control)).then_some(decoded)
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
    fn resampling_produces_display_sized_pixels_under_a_size_bearing_identity() {
        // A 4x4 image split left-black / right-white, downscaled to 2x2: separable resampling of a
        // vertically uniform image keeps each column band's colour, so the result is provably a
        // resample and not a crop.
        let mut rgba = Vec::new();
        for _ in 0..4 {
            for x in 0..4 {
                let value = if x < 2 { 0 } else { 255 };
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let task = InlineImageScaleTask {
            occurrence_id: 5,
            content_key: "image:abc".to_owned(),
            rgba: Arc::from(rgba),
            width_px: 4,
            height_px: 4,
            display_width_px: 2,
            display_height_px: 2,
        };
        let scaled = scale_inline_image(&task);
        assert_eq!(scaled.occurrence_id, 5);
        assert_eq!(scaled.content_key, "image:abc");
        assert_eq!(scaled.key, "image:abc@2x2");
        assert_eq!((scaled.width_px, scaled.height_px), (2, 2));
        assert_eq!(scaled.rgba.len(), 2 * 2 * 4);
        assert!(scaled.rgba[0] < 64, "the left half stays dark");
        assert!(scaled.rgba[4] > 192, "the right half stays light");

        // The same content at a second display size is a second texture identity, never a reuse.
        let larger = scale_inline_image(&InlineImageScaleTask {
            display_width_px: 8,
            display_height_px: 8,
            ..task.clone()
        });
        assert_ne!(larger.key, scaled.key);
        assert_eq!(larger.rgba.len(), 8 * 8 * 4);

        // A request already at the native size shares the decode's buffer rather than copying it.
        let identity = scale_inline_image(&InlineImageScaleTask {
            display_width_px: 4,
            display_height_px: 4,
            ..task.clone()
        });
        assert_eq!(identity.key, "image:abc@4x4");
        assert!(Arc::ptr_eq(&identity.rgba, &task.rgba));
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
        assert_eq!(
            detect_local_image_path_candidates(r"[Image: source: C:\tmp\image.svg]")[0].path,
            r"C:\tmp\image.svg",
            "svg joined the admissible extensions with the 2026-08-02 static-raster slice"
        );
        for rejected in [
            r"[Image: source: relative\image.png]",
            r"[Image: source: \\server\share\image.png]",
            r"[Image: source: C:\tmp\image.bmp]",
            r#"[Image: source: "C:\tmp\unterminated image.png]"#,
            r"prefixXC:\tmp\image.png",
        ] {
            assert!(
                detect_local_image_path_candidates(rejected).is_empty(),
                "unexpected candidate in {rejected:?}"
            );
        }
    }

    /// PIN (boundary defect, 2026-08-02): a path is opened by any character that could not be
    /// continuing a token, and closed by any closing delimiter — in every script, not only ASCII.
    /// The report that produced this: a path inside full-width parentheses in CJK prose was
    /// invisible to the detector from both ends at once, because the byte before `D` was a UTF-8
    /// continuation byte and the byte after `.png` was another one.
    #[test]
    fn path_candidates_open_after_any_non_token_character_and_close_at_any_closing_delimiter() {
        for accepted in [
            "（D:\\Developer\\BetterTerminal\\layout-preview.png）",
            "见图（D:\\Developer\\BetterTerminal\\layout-preview.png）",
            "「D:\\Developer\\BetterTerminal\\layout-preview.png」",
            "【D:\\Developer\\BetterTerminal\\layout-preview.png】",
            "路径：D:\\Developer\\BetterTerminal\\layout-preview.png",
            "(D:\\Developer\\BetterTerminal\\layout-preview.png)",
            "<D:\\Developer\\BetterTerminal\\layout-preview.png>",
            "《D:\\Developer\\BetterTerminal\\layout-preview.png》",
            "“D:\\Developer\\BetterTerminal\\layout-preview.png”",
            "图\u{3000}D:\\Developer\\BetterTerminal\\layout-preview.png",
        ] {
            let candidates = detect_local_image_path_candidates(accepted);
            assert_eq!(
                candidates
                    .iter()
                    .map(|candidate| candidate.path.as_str())
                    .collect::<Vec<_>>(),
                vec![r"D:\Developer\BetterTerminal\layout-preview.png"],
                "a path in {accepted:?} must be seen whole"
            );
            assert_eq!(
                &accepted[candidates[0].byte_start..candidates[0].byte_end],
                candidates[0].path,
                "the span must address the path text exactly in {accepted:?}"
            );
        }
        for rejected in [
            // A drive prefix that continues a token is a suffix of that token, never a path. The
            // `/` case is load-bearing: it is what keeps a `file://` URI out of the native scan.
            "file:///D:/Developer/BetterTerminal/layout-preview.png",
            "见D:\\a\\b.png",
            "v1.D:\\a\\b.png",
            "x-D:\\a\\b.png",
            "x_D:\\a\\b.png",
            "sub\\D:\\a\\b.png",
        ] {
            assert!(
                detect_local_image_path_candidates(rejected).is_empty(),
                "unexpected native candidate in {rejected:?}"
            );
        }
        // A quoted path keeps every delimiter it contains; quoting is how a filename that really
        // ends in `）` is spelled.
        assert_eq!(
            detect_local_image_path_candidates("（\"D:\\a\\b（1）.png\"）")[0].path,
            "D:\\a\\b（1）.png"
        );
    }

    /// PIN (file:// peek admission, 2026-08-02): a URI reaching the peek is decoded as a URI —
    /// per-segment percent decoding, authority checked — and then passes exactly the gates a
    /// printed path passes. Nothing here loosens what may be previewed; it only widens how the
    /// same file may be *named*.
    #[test]
    fn file_uris_resolve_to_local_image_paths_under_the_same_admission_gates() {
        for (uri, expected) in [
            (
                "file:///D:/Developer/BetterTerminal/layout-preview.png",
                r"D:\Developer\BetterTerminal\layout-preview.png",
            ),
            ("file:///D:/x%20y.png", r"D:\x y.png"),
            ("file:///D:/%E5%9B%BE%E7%89%87.PNG", r"D:\图片.PNG"),
            // A `%2F` is a literal slash inside one name, not a separator; the name simply will
            // not exist, which the worker discovers as it discovers every other absence.
            ("file:///D:/a%2Fb.png", "D:\\a/b.png"),
            ("FILE:///D:/a.png", r"D:\a.png"),
            ("file://localhost/D:/a.png", r"D:\a.png"),
            ("file:///D:/a.png#anchor", r"D:\a.png"),
            ("file:///D:/a.png?v=2", r"D:\a.png"),
        ] {
            assert_eq!(
                file_uri_to_local_image_path(uri),
                Some(PathBuf::from(expected)),
                "{uri:?}"
            );
        }
        for rejected in [
            // A remote share is not the local image peek's business.
            "file://host/share/a.png",
            "file://192.168.0.2/pics/a.png",
            // The same allowlist and drive-root gate printed paths meet.
            "file:///D:/notes.txt",
            "file:///D:/a.bmp",
            "file:///etc/a.png",
            "file:///a.png",
            // Not a file URI, or not a URI at all.
            "https://example.test/a.png",
            "file://",
            "file://host",
            // Malformed escapes and names no filesystem may hold.
            "file:///D:/a%zz.png",
            "file:///D:/a%2.png",
            "file:///D:/a%00.png",
            "file:///D://a.png",
        ] {
            assert_eq!(file_uri_to_local_image_path(rejected), None, "{rejected:?}");
        }
    }

    /// PIN (file:// peek admission, 2026-08-02): bare URI text in a line is found the way path
    /// text is, and reports the resolved path under the span of the URI that must be hovered.
    #[test]
    fn file_uri_candidates_are_found_in_prose_and_carry_the_resolved_path() {
        let text = "see file:///D:/a/layout-preview.png, and （file:///D:/b.png）, not \
                    file:///D:/notes.txt or xfile:///D:/c.png";
        let candidates = detect_local_image_uri_candidates(text);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| (
                    candidate.path.as_str(),
                    &text[candidate.byte_start..candidate.byte_end]
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    r"D:\a\layout-preview.png",
                    "file:///D:/a/layout-preview.png"
                ),
                (r"D:\b.png", "file:///D:/b.png"),
            ]
        );
        // The two shapes never claim the same text: the `D:/…` inside a URI is not a native path.
        assert!(detect_local_image_path_candidates(text).is_empty());
        assert_eq!(
            detect_peek_image_candidates(text).len(),
            2,
            "the peek reads one line once, through both shapes"
        );
    }

    #[test]
    fn svg_local_path_decodes_through_the_rasterizer_at_intrinsic_size() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "betterterminal-inline-svg-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("probe.svg");
        std::fs::write(
            &path,
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="6">
                <rect x="0" y="0" width="8" height="6" fill="#00ff00"/>
            </svg>"##,
        )
        .unwrap();

        let mut decoder = InlineImageDecoder::default();
        let decoded = decoder
            .decode(InlineImageTask {
                occurrence_id: 71,
                source: InlineImageSource::LocalPath(path.clone()),
            })
            .unwrap();
        assert_eq!((decoded.width_px, decoded.height_px), (8, 6));
        assert!(!decoded.animated);
        assert_eq!(&decoded.rgba[..4], &[0, 255, 0, 255]);
        assert!(decoded.key.starts_with("image:"));

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
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
