use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bt_transcript::paths::is_local_absolute_path;
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
/// Bound on the `file://` URI an OSC 7 report may carry. A Windows extended-length path tops out
/// at 32767 UTF-16 units, but a working directory that percent-encodes past this is not a
/// directory this terminal will resolve relative text against; the report is dropped whole rather
/// than truncated into a different directory.
const MAX_OSC_7_URI_BYTES: usize = 4 * 1024;

/// How much text one `OSC 9` / `OSC 777;notify` payload may carry.
///
/// A kilobyte, and it is a bound on a *message a person reads*: Windows renders a toast in two
/// lines and clips the rest, so anything past this could not be read even if it were kept. The
/// number matters because these two sequences are the first ones whose payload is arbitrary text
/// from the far end of a pipe — everything else this scanner holds is a URI, a marker letter or a
/// pair of integers.
const MAX_OSC_NOTIFICATION_BYTES: usize = 1024;

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
    decode_image_bytes_within(bytes, MAX_INLINE_IMAGE_RGBA_BYTES)
}

/// The container walk, under a caller's pixel budget.
///
/// The budget is a parameter and not a constant because there are two callers
/// with two honest answers: an inline image nobody asked for gets
/// [`MAX_INLINE_IMAGE_RGBA_BYTES`], and a background picture somebody chose
/// through a chooser gets [`MAX_BACKGROUND_IMAGE_RGBA_BYTES`]. Everything else
/// about the walk — which containers are admitted, how APNG is detected, the
/// dimension check that follows the decode — is one reading for both, which is
/// the reason this is a parameter rather than a second function.
fn decode_image_bytes_within(
    bytes: &[u8],
    max_rgba_bytes: u64,
) -> Result<DecodedImagePayload, InlineImageDecodeError> {
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
    limits.max_alloc = Some(max_rgba_bytes);
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
    if width_px == 0 || height_px == 0 || expected > max_rgba_bytes || expected != rgba.len() as u64
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

// ── the window's ground picture, which is not an inline image ──────────────
//
// **The background borrowed the inline decoder's gates and inherited the wrong
// budget** (user report 2026-08-18: a 24 MP camera JPEG raised "inline image
// exceeds its decode limit" from a settings row). §7.1.6c-4b's ruling was
// "decode with the one decoder this repo has, limits and all", and the limits
// were the honest ones *for an inline image*: a picture a shell wrote into a
// scrollback arrives unasked, may arrive a hundred times in a screenful, and
// costs a texture each — 8 MiB of file and 64 MiB of pixels is a generous
// allowance for something nobody chose.
//
// A wallpaper is the opposite object in every one of those respects. There is
// exactly one per window, it was chosen by hand through a modal chooser, it is
// uploaded once and it is never a surprise. So it gets its own budget, and the
// budget is stated against what it actually costs rather than against what an
// inline image costs.

/// What a background picture's *file* may weigh.
///
/// 64 MiB, which is the whole of the "is this a picture or a mistake" question
/// at this size: the largest ordinary camera JPEG in circulation — a 100 MP
/// medium-format frame at maximum quality — is under 60 MB, and everything
/// above that is a scan, a render or a wrong file. The read is transient (it is
/// dropped the moment the pixels exist) and it happens on a worker, so the cost
/// of being generous here is a moment of memory on a thread nobody is waiting
/// on.
pub const MAX_BACKGROUND_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// What a background picture may decode to before it is resampled.
///
/// 768 MiB of RGBA, which is 192 megapixels. It is deliberately far above
/// anything the *upload* will ever see — [`background_target_size`] cuts the
/// picture down to the largest monitor before it leaves this module — because
/// this bound is not a resource budget, it is the line between a large photo
/// and a decompression bomb. The resource budget is the resample target.
pub const MAX_BACKGROUND_IMAGE_RGBA_BYTES: u64 = 768 * 1024 * 1024;

/// Why a chosen background picture is not on screen.
///
/// A type of its own rather than [`InlineImageDecodeError`] because the failure
/// is reported on a **settings row**, and that row must not speak about inline
/// images: "inline image exceeds its decode limit" is true of a mechanism the
/// reader of that row has never heard of, and it names a limit that is not the
/// one that was applied (user report 2026-08-18).
///
/// **The variants carry the facts and not a sentence**, which is what lets the
/// window say them in either language: `bt_app::i18n::background_picture_refused`
/// matches on this type. The `Display` below is the developer-facing reading —
/// what a log line or an `anyhow` chain gets — and it is held to the same rule
/// by `a_refused_background_picture_never_speaks_of_inline_images`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackgroundImageError {
    /// Not a drive-rooted path spelled like one of the formats this build
    /// decodes.
    InvalidPath,
    Io(String),
    /// The file is bigger than [`MAX_BACKGROUND_IMAGE_BYTES`]. Both numbers are
    /// carried so the sentence can name the limit it applied — an error that
    /// says only "too large" leaves the reader with no way to tell a 70 MB file
    /// from a 700 MB one.
    TooLarge {
        bytes: u64,
        limit: u64,
    },
    /// A container this build has no decoder for.
    UnsupportedFormat,
    /// The decoder read the header and then refused the body.
    Decode(String),
    /// Zero-sized, or past [`MAX_BACKGROUND_IMAGE_RGBA_BYTES`].
    InvalidDimensions,
}

impl fmt::Display for BackgroundImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => formatter.write_str("not a picture file this window can open"),
            Self::Io(error) => write!(formatter, "could not be read: {error}"),
            Self::TooLarge { bytes, limit } => write!(
                formatter,
                "is {} and a background picture may be up to {}",
                mebibytes(*bytes),
                mebibytes(*limit)
            ),
            Self::UnsupportedFormat => {
                formatter.write_str("is not a picture format this build decodes")
            }
            Self::Decode(error) => write!(formatter, "could not be decoded: {error}"),
            Self::InvalidDimensions => {
                formatter.write_str("has no pixels, or more of them than this build will decode")
            }
        }
    }
}

impl std::error::Error for BackgroundImageError {}

/// A byte count as a reader of a file listing would see it.
///
/// Public because the two sentences that quote a byte count — this module's
/// developer-facing `Display` and the window's own bilingual card — have to
/// quote the *same* number in the same units, and a second rounding written
/// beside the second sentence is how "64 MB" and "67 MB" come to name one limit.
#[must_use]
pub fn mebibytes(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// The size a background picture is resampled to before it is uploaded.
///
/// **The ceiling is the largest monitor's pixel size, and never the picture's
/// own.** The ground quad covers the window and nothing else, so the most
/// texels that can ever be resolved from it is the number of physical pixels
/// the window can occupy — which is bounded, for the life of the process, by
/// the biggest screen attached to it. A 6000x4000 photo on a 3840-wide monitor
/// has three quarters of its pixels resolved by the sampler into texels nobody
/// can see, and it costs 96 MiB of upload to not show them.
///
/// The window's *current* size was the other candidate and is the wrong one: a
/// window is resized and maximised constantly, and a ceiling taken from it
/// would either re-decode the file on every drag or lock the picture to
/// whatever shape the window happened to have when it was chosen. The monitor
/// does not change while a frame is being dragged.
///
/// Downscale only — a picture smaller than the ceiling is uploaded as it is,
/// because upsampling on the CPU produces exactly what the sampler would have
/// produced from the smaller texture and charges memory for it.
///
/// **What this changes for `Tile`**, stated rather than discovered: a tile
/// larger than the largest monitor is shrunk to it, so a picture that used to
/// show one copy and a sliver of a second now shows exactly one. Every tile
/// smaller than a screen — which is every picture anybody tiles — is untouched.
#[must_use]
pub fn background_target_size(image: (u32, u32), ceiling: (u32, u32)) -> (u32, u32) {
    let (width, height) = image;
    let (max_width, max_height) = ceiling;
    if width == 0 || height == 0 || max_width == 0 || max_height == 0 {
        return image;
    }
    if width <= max_width && height <= max_height {
        return image;
    }
    // One scale for both axes: the aspect ratio is the picture's own and the
    // three fits are the only things allowed to change it.
    let scale = f64::min(
        f64::from(max_width) / f64::from(width),
        f64::from(max_height) / f64::from(height),
    );
    let shrink = |side: u32, cap: u32| ((f64::from(side) * scale).round() as u32).clamp(1, cap);
    (shrink(width, max_width), shrink(height, max_height))
}

/// Read the picture named for the window's ground, and hand back the texture it
/// will be uploaded as.
///
/// Worker work, always (§7.1.6c-4d, user report 2026-08-18). 4b decoded on the
/// event thread and argued it: the two moments this runs are startup and a
/// modal chooser closing, and "both already have somebody waiting". The
/// argument holds for a screenshot and fails for a photograph — a 24 MP JPEG is
/// a fifth of a second of decode plus a resample on top, and a window that
/// stops answering the keyboard for a quarter of a second is not a window that
/// was being polite to somebody waiting. The row applies immediately; the
/// picture lands a beat later.
///
/// The returned key carries the resample target, so a picture that comes back
/// at a different size is a different texture rather than a stale one — the
/// same rule [`display_texture_key`] states for an inline image at a zoom.
pub fn decode_background_image(
    path: &Path,
    ceiling: (u32, u32),
) -> Result<DecodedInlineImage, BackgroundImageError> {
    if !is_admissible_local_image_path(path) {
        return Err(BackgroundImageError::InvalidPath);
    }
    let mut file = File::open(path).map_err(|error| BackgroundImageError::Io(error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| BackgroundImageError::Io(error.to_string()))?;
    if !metadata.is_file() {
        return Err(BackgroundImageError::InvalidPath);
    }
    if metadata.len() > MAX_BACKGROUND_IMAGE_BYTES {
        return Err(BackgroundImageError::TooLarge {
            bytes: metadata.len(),
            limit: MAX_BACKGROUND_IMAGE_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(MAX_BACKGROUND_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| BackgroundImageError::Io(error.to_string()))?;
    if bytes.len() as u64 > MAX_BACKGROUND_IMAGE_BYTES {
        return Err(BackgroundImageError::TooLarge {
            bytes: bytes.len() as u64,
            limit: MAX_BACKGROUND_IMAGE_BYTES,
        });
    }
    let payload =
        decode_image_bytes_within(&bytes, MAX_BACKGROUND_IMAGE_RGBA_BYTES).map_err(|error| {
            match error {
                InlineImageDecodeError::UnsupportedFormat => {
                    BackgroundImageError::UnsupportedFormat
                }
                InlineImageDecodeError::InvalidDimensions => {
                    BackgroundImageError::InvalidDimensions
                }
                InlineImageDecodeError::TooLarge => BackgroundImageError::TooLarge {
                    bytes: bytes.len() as u64,
                    limit: MAX_BACKGROUND_IMAGE_BYTES,
                },
                other => BackgroundImageError::Decode(other.to_string()),
            }
        })?;
    let (width_px, height_px) =
        background_target_size((payload.width_px, payload.height_px), ceiling);
    let scaled = scale_inline_image(&InlineImageScaleTask {
        occurrence_id: 0,
        content_key: payload.key,
        rgba: payload.rgba,
        width_px: payload.width_px,
        height_px: payload.height_px,
        display_width_px: width_px,
        display_height_px: height_px,
    });
    Ok(DecodedInlineImage {
        occurrence_id: 0,
        key: scaled.key,
        rgba: scaled.rgba,
        width_px,
        height_px,
        animated: payload.animated,
    })
}

fn is_admissible_local_image_path(path: &Path) -> bool {
    is_local_absolute_path(path) && has_admissible_image_extension(path)
}

/// Whether a name is spelled like a picture this build can show.
///
/// Public because it is the *routing* question as well as the detection one: a
/// files row being activated has to decide between the preview pane and the
/// system's own handler, and deciding it against a second copy of this list is
/// how a tree comes to send a `.webp` somewhere the terminal would not.
pub fn has_admissible_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg"
            )
        })
}

/// How a reference is *spelled* on the line — the one property that decides whether it may ever
/// grow an inline band, kept on the candidate so the verification layer never has to re-derive it.
///
/// The distinction is older than this type: a printed native path is the file's name in the flow
/// and has always been band-eligible, while a `file://` URI is a reference *to* a file and never
/// was. Both are verified and both wear the resting underline; only `Native` reaches
/// `projected_inline_image`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageReferenceShape {
    /// A drive-rooted path, or a relative reference resolved against the OSC 7 working directory.
    Native,
    /// A `file://` URI, whose printed form is not its path.
    Uri,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalImagePathCandidate {
    pub path: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub shape: ImageReferenceShape,
}

/// The drive-rooted printed paths of one line that name a picture this build can show.
///
/// Where a path begins and ends is not asked here — that question belongs to one table
/// ([`bt_transcript::paths::detect_absolute_path_candidates`]) shared with everything else in this
/// window that reads a printed path, because two copies of a boundary rule are two opinions about
/// what the pointer is standing on. What is asked here, and only here, is the **extension
/// allowlist**: this is the scan that decides what may become a picture, and admitting a `.txt`
/// would put a text file in front of an image decoder. Existence, size, content format and decode
/// remain worker-only.
pub fn detect_local_image_path_candidates(text: &str) -> Vec<LocalImagePathCandidate> {
    bt_transcript::paths::detect_absolute_path_candidates(text)
        .into_iter()
        .filter_map(|candidate| {
            let path = candidate.text(text);
            is_admissible_local_image_path(Path::new(path)).then(|| LocalImagePathCandidate {
                path: path.to_owned(),
                byte_start: candidate.byte_start,
                byte_end: candidate.byte_end,
                shape: ImageReferenceShape::Native,
            })
        })
        .collect()
}

/// The relative references of one line that name a picture this build can show, each still spelled
/// **exactly as printed**: a relative reference names nothing until it is joined to a directory, and
/// this terminal only ever learns a directory by being told one (OSC 7).
/// `resolve_relative_image_path` is the join; `detect_inline_image_candidates` is where the two meet.
///
/// Which text is a relative reference at all — anchored `./`, `../`, or bare with a separator, and
/// where it stops — is [`bt_transcript::paths::detect_relative_path_candidates`]'s answer, shared
/// with every other reader of a printed path in this window. The **extension allowlist** is what
/// this scan adds, and it is passed *into* the scan rather than applied to its result because
/// admission decides how far the cursor moves: a candidate this reading refuses gives up a single
/// character, so a reference may still begin one character later inside the same token.
pub fn detect_relative_image_path_candidates(text: &str) -> Vec<LocalImagePathCandidate> {
    bt_transcript::paths::detect_relative_path_candidates(text, &|candidate| {
        has_admissible_image_extension(Path::new(candidate))
    })
    .into_iter()
    .map(|candidate| LocalImagePathCandidate {
        path: candidate.text(text).to_owned(),
        byte_start: candidate.byte_start,
        byte_end: candidate.byte_end,
        shape: ImageReferenceShape::Native,
    })
    .collect()
}

/// Join a relative candidate onto an authoritative working directory and normalize it lexically.
///
/// Lexical by construction: `..` pops the component to its left instead of asking the filesystem
/// what a symlink underneath it resolves to. That is exactly what keeps this on the event thread —
/// existence, file kind, size and decode stay worker-only, precisely as they are for a printed
/// absolute path. The result meets the same `is_admissible_local_image_path` gate every other
/// shape meets, so resolution widens how a file may be *named* and never what may be previewed.
///
/// `..` that would climb past the drive root names nothing a filesystem can hold, and a join that
/// lands on the bare drive root names a directory rather than a file; both are simply not
/// candidates.
pub fn resolve_relative_image_path(working_directory: &Path, relative: &str) -> Option<PathBuf> {
    bt_transcript::paths::resolve_relative_reference(working_directory, relative)
        .filter(|path| is_admissible_local_image_path(path))
}

/// Every image reference one line of text offers **inline admission**: native drive-rooted paths,
/// plus relative references — anchored (`./`, `../`) or bare with a separator — resolved against
/// `working_directory`.
///
/// `working_directory` is `None` for any session whose shell has never reported one over OSC 7,
/// and then relative text yields no candidates at all. That is the ruling, not a limitation to be
/// worked around: a relative path without an authoritative directory is a guess, and this terminal
/// does not guess where a line of text was printed from.
pub fn detect_inline_image_candidates(
    text: &str,
    working_directory: Option<&Path>,
) -> Vec<LocalImagePathCandidate> {
    let mut candidates = detect_local_image_path_candidates(text);
    if let Some(working_directory) = working_directory {
        candidates.extend(resolved_relative_image_candidates(text, working_directory));
    }
    candidates
}

/// The relative candidates of one line — anchored and bare alike — each carrying its **resolved**
/// absolute path under the span of the relative text that must be hovered, the convention
/// `detect_local_image_uri_candidates` already established for a reference whose printed form is
/// not its path.
fn resolved_relative_image_candidates(
    text: &str,
    working_directory: &Path,
) -> Vec<LocalImagePathCandidate> {
    detect_relative_image_path_candidates(text)
        .into_iter()
        .filter_map(|candidate| {
            let path = resolve_relative_image_path(working_directory, &candidate.path)?;
            Some(LocalImagePathCandidate {
                path: path.to_string_lossy().into_owned(),
                ..candidate
            })
        })
        .collect()
}

/// The `file://` URIs of one line that name an admissible local image, spelled as URIs.
///
/// Kept apart from `detect_local_image_path_candidates` because the two shapes earn different
/// presentations: a printed native path grows an inline band on the primary screen, a URI never
/// does. A URI is a reference to a file, not the file's name in the flow, so it answers hover and
/// nothing else. `path` on the returned candidate is therefore the **resolved** local path, while
/// the byte span covers the URI text that must be hovered to reach it.
pub fn detect_local_image_uri_candidates(text: &str) -> Vec<LocalImagePathCandidate> {
    bt_transcript::paths::detect_file_uri_candidates(text)
        .into_iter()
        .filter_map(|candidate| {
            let path = file_uri_to_local_image_path(candidate.text(text))?;
            Some(LocalImagePathCandidate {
                path: path.to_string_lossy().into_owned(),
                byte_start: candidate.byte_start,
                byte_end: candidate.byte_end,
                shape: ImageReferenceShape::Uri,
            })
        })
        .collect()
}

/// Every image one line of text offers the hover peek: everything inline admission reads
/// (`detect_inline_image_candidates` — native drive-rooted paths and resolved relative paths) plus
/// `file://` URIs, which only ever peek.
///
/// No two shapes can claim the same text. A URI's embedded `D:/…` and its `./…`-looking interior
/// are both preceded by `/`, which `is_path_tail_char` rejects as an opening; a native path's own
/// `\.\` is preceded by `\` for the same reason. The bare relative shape, which has no prefix of
/// its own to be recognized by, is held off the other two by `is_relative_image_reference`: a
/// candidate carrying a `:` is somebody else's (a drive's, a scheme's), and one opening with a
/// separator is the remainder of somebody else's.
pub fn detect_peek_image_candidates(
    text: &str,
    working_directory: Option<&Path>,
) -> Vec<LocalImagePathCandidate> {
    let mut candidates = detect_inline_image_candidates(text, working_directory);
    candidates.extend(detect_local_image_uri_candidates(text));
    candidates
}

/// Resolve a `file://` URI to the local image path it names, or `None` when it names something
/// this terminal will not preview.
///
/// The URI is a *reference*; the gates it must pass are the same ones printed path text passes
/// (`is_admissible_local_image_path`: drive-rooted and an allowlisted image extension), so no
/// shape buys a privilege another lacks. Existence, size, and decode stay worker-side as always.
///
/// A peeked image URI is text some process printed into the flow, so its authority must be empty
/// or `localhost` and nothing else — this machine's hostname is deliberately not consulted here.
/// A trailing slash names a directory, which is never an image.
pub fn file_uri_to_local_image_path(uri: &str) -> Option<PathBuf> {
    bt_transcript::paths::file_uri_to_local_reference(uri)
        .filter(|path| has_admissible_image_extension(path))
}

/// Decode a `file://` URI to the local path it names, applying only the shape gate every local
/// reference shares (`is_local_absolute_path`) and no extension allowlist.
///
/// This is the URI machinery itself, shared by the image peek above and by the OSC 7 working
/// directory — a directory has no extension, so the image gate must not be wired into the decoder.
///
/// Resolution is per URI segment: each is percent-decoded on its own and the results are joined
/// with `\`. That is what RFC 3986 means — a `%2F` inside a segment decodes to a literal `/` in a
/// filename, never to a separator — and it costs nothing, since such a name simply fails to exist.
/// A single **trailing** empty segment is a directory's trailing slash rather than an empty name
/// (`file:///D:/src/` and `file:///D:/` both name directories); an interior one (`file:///D://a`)
/// stays rejected.
///
/// A non-empty authority is accepted only when it is `localhost` or `local_host`, this machine's
/// own name — the two spellings of "this host" that a file URI has. Anything else is a remote
/// share (`file://server/share/a.png`), which no local read may follow. Callers that must not
/// honour a hostname at all pass `None`.
///
/// **This one accepts a POSIX root as well as a drive letter**, and that is what an OSC 7 report
/// from a shell running inside WSL looks like: `file:///home/weiyi/src`. The directory a shell is
/// standing in is a fact about *that shell's* filesystem, and a Linux shell on this machine has no
/// drive letter to offer — `wslpath -w` would answer `\\wsl.localhost\<distro>\home\weiyi`, a UNC
/// whose authority this very function is obliged to reject as a remote share. Refusing the POSIX
/// spelling therefore does not keep a foreign path out; it only makes the most common directory in
/// WSL unnameable, so a WSL pane could never say where it is.
///
/// What the wider door does **not** do is make such a path resolvable as a Windows one. Everything
/// downstream that joins a relative reference onto this directory asks `is_local_absolute_path`
/// first (see [`resolve_relative_reference`]), which still means "drive-rooted"; a POSIX directory
/// therefore leaves relative image text undetected, which is the standing rule for a directory this
/// terminal cannot vouch for (`docs/shell-integration.md` §34-35) rather than a new exception. The
/// image peek keeps the strict reading too — see [`file_uri_to_local_image_path`].
pub fn file_uri_to_local_path(uri: &str, local_host: Option<&str>) -> Option<PathBuf> {
    bt_transcript::paths::decode_file_uri(
        uri,
        local_host,
        bt_transcript::paths::TrailingSlash::Directory,
        bt_transcript::paths::Rooting::DriveOrPosixRoot,
    )
}

/// This machine's name — the one authority a `file://` URI may carry besides none and `localhost`.
///
/// Read once: a machine does not rename itself inside one terminal session, and the OSC 7 path
/// runs on the event thread.
pub fn local_host_name() -> Option<&'static str> {
    static LOCAL_HOST: OnceLock<Option<String>> = OnceLock::new();
    LOCAL_HOST
        .get_or_init(|| {
            std::env::var("COMPUTERNAME")
                .ok()
                .filter(|name| !name.is_empty())
        })
        .as_deref()
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum InlineImageStreamAction {
    Bytes(Vec<u8>),
    Image(Vec<u8>),
    ShellIntegration(ShellIntegrationMarker),
    Progress(Option<crate::session::ProgressState>),
    /// One desktop notification a program asked for, over `OSC 9` or `OSC 777;notify`.
    Notification(crate::session::TerminalNotification),
    /// One OSC 7 report: the `file://` URI bytes the shell named its working directory with. An
    /// empty payload is the report "I no longer have one to give", which is a fact of its own and
    /// therefore still an action.
    WorkingDirectory(Vec<u8>),
    TooLarge,
}

/// FinalTerm Command Status markers used by PowerShell shell integration. Parameters after the
/// command letter do not affect region ownership; `D` retains a numeric exit status for session
/// attention facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellIntegrationMarker {
    PromptStart,
    CommandStart,
    CommandExecuted,
    CommandFinished { exit_code: Option<i32> },
}

/// The OSC sequences whose payload this scanner keeps whole and reads at the terminator.
///
/// Four sequences and one state, because all four are the same *machine*: hold the bytes between
/// the prefix and the terminator, drop the C0 controls that can never be part of a payload, stop
/// retaining past a limit, and hand what survives to one function at the end. Only that limit and
/// that function differ, so both hang off this enum rather than off four copies of the state —
/// a fifth text OSC is a variant and two match arms, not another pair of states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextOsc {
    /// `OSC 133;` — FinalTerm's command-status markers.
    ShellIntegration,
    /// `OSC 7;` — the working directory the shell reports.
    WorkingDirectory,
    /// `OSC 9;` — ConEmu's numbered subcommand slot (of which this terminal implements `4`,
    /// progress) and, for a body that is not a number, iTerm2's desktop notification.
    Osc9,
    /// `OSC 777;` — urxvt's `notify` extension, the only verb of that sequence anybody sends.
    Osc777,
}

impl TextOsc {
    /// How many payload bytes are kept before the sequence is abandoned.
    ///
    /// Two answers rather than one: a URI is a path and paths are long, while a marker, a
    /// progress report and a notification are all things a person reads. The limit is not a
    /// truncation — a payload that reaches it is dropped whole, because half a URI names a
    /// different directory and half a message is a message nobody wrote.
    fn payload_limit(self) -> usize {
        match self {
            Self::WorkingDirectory => MAX_OSC_7_URI_BYTES,
            Self::ShellIntegration => 128,
            Self::Osc9 | Self::Osc777 => MAX_OSC_NOTIFICATION_BYTES,
        }
    }

    fn finish(self, actions: &mut Vec<InlineImageStreamAction>, payload: Vec<u8>, oversized: bool) {
        match self {
            Self::ShellIntegration => finish_shell_integration(actions, &payload, oversized),
            Self::WorkingDirectory => finish_working_directory(actions, payload, oversized),
            Self::Osc9 => finish_osc_9(actions, &payload, oversized),
            Self::Osc777 => finish_osc_777(actions, &payload, oversized),
        }
    }
}

#[derive(Debug)]
enum StreamState {
    Ground,
    Escape,
    OscPrefix {
        held: Vec<u8>,
    },
    OscPass,
    InlineFile(InlineFileCapture),
    AfterInlineEscape,
    Text {
        kind: TextOsc,
        payload: Vec<u8>,
        oversized: bool,
    },
    AfterTextEscape {
        kind: TextOsc,
        payload: Vec<u8>,
        oversized: bool,
    },
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
/// The OSC sequences Folio gives meaning to — `1337;File=` (inline image), `133;` (shell
/// integration), `9;` (ConEmu progress and iTerm2 notifications), `777;` (urxvt notifications)
/// and `7;` (working directory) — are swallowed by `vte::ansi::Performer` before they
/// reach `alacritty_terminal::Term`, so they are recognized here instead. This is the whole of the
/// vendor face for all five: nothing upstream is patched.
///
/// Recognition is by exact prefix, and every byte of every other sequence stays on its unchanged
/// path — a prefix that turns out not to match is emitted whole the moment it is ruled out.
/// Unlike a second unbounded `vte::Parser`, this can also stop retaining an oversized payload
/// before its terminator arrives.
///
/// **`7;` and `777;` share their first byte and do not collide**, because the decision is made on
/// the whole prefix and not on a leading character: `7` alone is still a candidate for both, `7;`
/// is exactly one of them, and `77` has already ruled the shorter one out.
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
                    if b"1337;".starts_with(body)
                        || b"133;".starts_with(body)
                        || b"9;".starts_with(body)
                        || b"777;".starts_with(body)
                        || b"7;".starts_with(body)
                    {
                        let text = match body {
                            b"133;" => Some(TextOsc::ShellIntegration),
                            b"7;" => Some(TextOsc::WorkingDirectory),
                            b"9;" => Some(TextOsc::Osc9),
                            b"777;" => Some(TextOsc::Osc777),
                            _ => None,
                        };
                        if let Some(kind) = text {
                            flush_bytes(&mut actions, &mut ordinary);
                            StreamState::Text {
                                kind,
                                payload: Vec::new(),
                                oversized: false,
                            }
                        } else if body == b"1337;" {
                            flush_bytes(&mut actions, &mut ordinary);
                            StreamState::InlineFile(InlineFileCapture::default())
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
                StreamState::Text {
                    kind,
                    mut payload,
                    mut oversized,
                } => match byte {
                    0x07 => {
                        kind.finish(&mut actions, payload, oversized);
                        StreamState::Ground
                    }
                    0x1b => StreamState::AfterTextEscape {
                        kind,
                        payload,
                        oversized,
                    },
                    0x18 | 0x1a => StreamState::Ground,
                    0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f => StreamState::Text {
                        kind,
                        payload,
                        oversized,
                    },
                    _ => {
                        if payload.len() < kind.payload_limit() {
                            payload.push(byte);
                        } else {
                            oversized = true;
                        }
                        StreamState::Text {
                            kind,
                            payload,
                            oversized,
                        }
                    }
                },
                StreamState::AfterTextEscape {
                    kind,
                    payload,
                    oversized,
                } => {
                    if byte == b'\\' {
                        kind.finish(&mut actions, payload, oversized);
                        StreamState::Ground
                    } else if byte == b']' {
                        // A nested OSC terminates the malformed outer sequence and begins a fresh
                        // one. This bounds recovery without manufacturing a semantic event.
                        StreamState::OscPrefix {
                            held: vec![0x1b, b']'],
                        }
                    } else if byte == 0x1b {
                        StreamState::AfterTextEscape {
                            kind,
                            payload,
                            oversized,
                        }
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

/// Report one terminated OSC 7.
///
/// A payload we had to truncate is reported as *no* directory rather than as the prefix we
/// happened to keep: the shell said it moved, and half a URI is a different directory, not a
/// smaller one. The session's response to an unresolvable report is to forget — see
/// `DualPlaneSession::set_reported_working_directory`.
fn finish_working_directory(
    actions: &mut Vec<InlineImageStreamAction>,
    payload: Vec<u8>,
    oversized: bool,
) {
    actions.push(InlineImageStreamAction::WorkingDirectory(if oversized {
        Vec::new()
    } else {
        payload
    }));
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
        b'D' => ShellIntegrationMarker::CommandFinished {
            exit_code: parameters
                .strip_prefix(b";")
                .filter(|value| !value.is_empty())
                .and_then(|value| std::str::from_utf8(value).ok())
                .and_then(|value| value.parse().ok()),
        },
        _ => return,
    };
    actions.push(InlineImageStreamAction::ShellIntegration(marker));
}

/// Split `payload` at its **first** separator, or report that it has none.
///
/// The first and not every one, and that is load-bearing in both places this is used: a
/// notification body is allowed to contain semicolons and an `OSC 777` title is not allowed to
/// swallow them (foot's rule, and the only one anybody implements — "Folio will split title from
/// body at the first ';', with any remaining ';' characters treated as part of body").
fn split_once(payload: &[u8], separator: u8) -> (&[u8], Option<&[u8]>) {
    match payload.iter().position(|byte| *byte == separator) {
        Some(index) => (&payload[..index], Some(&payload[index + 1..])),
        None => (payload, None),
    }
}

/// Read one terminated `OSC 9`, which is two protocols sharing a number.
///
/// ConEmu gave `OSC 9` a **numbered subcommand slot** — `1` sleep, `2` message box, `3` tab title,
/// `4` progress, and eight more — while iTerm2 gave the same sequence a single free-text field
/// that is a desktop notification. Windows Terminal implements only the progress arm; Ghostty is
/// the terminal that implements both, and its rule is the one written here because it is the only
/// one that can be stated in a sentence: **a body whose first field is entirely digits is
/// ConEmu's, and every other body is iTerm2's.**
///
/// Of ConEmu's numbers this terminal implements exactly one. The rest are *dropped rather than
/// notified about*: `OSC 9;9;C:\src` is a shell saying where it is, and raising a toast reading
/// "9;C:\src" would be this terminal inventing a message no program wrote. That is the whole cost
/// of the rule, and its price is stated in Ghostty's docs as well — a notification whose text
/// really does begin `12;` cannot be sent over `OSC 9`, and `OSC 777` is where it belongs.
fn finish_osc_9(actions: &mut Vec<InlineImageStreamAction>, payload: &[u8], oversized: bool) {
    if oversized {
        return;
    }
    let (head, rest) = split_once(payload, b';');
    if !head.is_empty() && head.iter().all(u8::is_ascii_digit) {
        if head == b"4"
            && let Some(progress) = parse_progress(rest.unwrap_or_default())
        {
            actions.push(InlineImageStreamAction::Progress(progress));
        }
        return;
    }
    push_notification(actions, None, payload);
}

/// Read one terminated `OSC 777`.
///
/// The sequence has a verb slot and exactly one verb anybody sends; WezTerm says so outright
/// ("only the notify extension is supported") and so does this. An unknown verb is dropped whole
/// rather than read as a title, because `OSC 777;foo;bar` is somebody else's extension and its
/// second field is not a name for anything.
///
/// `OSC 777;notify;<title>` with no body is a notification: a title is a message. `OSC 777;notify`
/// with neither is not — there is nothing in it to show.
fn finish_osc_777(actions: &mut Vec<InlineImageStreamAction>, payload: &[u8], oversized: bool) {
    if oversized {
        return;
    }
    let (verb, rest) = split_once(payload, b';');
    if verb != b"notify" {
        return;
    }
    let Some(rest) = rest else {
        return;
    };
    let (title, body) = split_once(rest, b';');
    push_notification(actions, Some(title), body.unwrap_or_default());
}

/// File one notification, if what arrived is one.
///
/// Two refusals, and neither is a policy about notifications — they are both "this is not a
/// message":
///
/// * **bytes that are not UTF-8.** Every other payload this scanner reads has a fallback that
///   means something (an unreadable URI is "no directory"), but there is no such thing as a
///   half-decoded sentence to show a person, and lossy decoding would put replacement characters
///   into a toast.
/// * **nothing to say.** A sequence carrying neither a title nor a body is a notification with no
///   content; the application would raise a toast that is a blank rectangle.
///
/// An empty title with a body is not that case — it is `OSC 9`'s ordinary shape said in `OSC 777`,
/// and the title falls back to the pane's own name exactly as `OSC 9`'s absent one does.
fn push_notification(
    actions: &mut Vec<InlineImageStreamAction>,
    title: Option<&[u8]>,
    body: &[u8],
) {
    let title = match title {
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(_) => return,
        },
        None => None,
    };
    let Ok(body) = std::str::from_utf8(body) else {
        return;
    };
    let title = title.filter(|text| !text.is_empty());
    if title.is_none() && body.is_empty() {
        return;
    }
    actions.push(InlineImageStreamAction::Notification(
        crate::session::TerminalNotification {
            title: title.map(str::to_owned),
            body: body.to_owned(),
        },
    ));
}

fn parse_progress(payload: &[u8]) -> Option<Option<crate::session::ProgressState>> {
    let mut fields = payload.split(|byte| *byte == b';');
    let status = std::str::from_utf8(fields.next()?)
        .ok()?
        .parse::<u8>()
        .ok()?;
    let percentage = match fields.next() {
        Some(value) => Some(parse_progress_percentage(value)?),
        None => None,
    };
    if fields.next().is_some() {
        return None;
    }
    match status {
        0 => Some(None),
        1 => Some(Some(crate::session::ProgressState::Normal(percentage?))),
        2 => Some(Some(crate::session::ProgressState::Error(percentage))),
        3 => Some(Some(crate::session::ProgressState::Indeterminate)),
        4 => Some(Some(crate::session::ProgressState::Paused(percentage))),
        _ => None,
    }
}

fn parse_progress_percentage(value: &[u8]) -> Option<u8> {
    let value = std::str::from_utf8(value).ok()?.parse::<i64>().ok()?;
    Some(value.clamp(0, 100) as u8)
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
                shape: ImageReferenceShape::Native,
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
            detect_peek_image_candidates(text, None).len(),
            2,
            "the peek reads one line once, through both shapes"
        );
    }

    /// PIN (relative path ruling, 2026-08-03 (a)): OSC 7 is read off the byte stream at the same
    /// prefilter seam OSC 133 and OSC 1337 are read at — reassembled across arbitrary chunk
    /// boundaries, under either terminator, with the surrounding bytes untouched.
    #[test]
    fn osc_7_reports_its_working_directory_uri_across_chunks_and_both_terminators() {
        let mut scanner = Osc1337Scanner::default();
        let mut actions = Vec::new();
        for chunk in [
            b"before\x1b]".as_slice(),
            b"7;file:///D:/a".as_slice(),
            b"/b\x07mid\x1b]7;file:///D:/c\x1b".as_slice(),
            b"\\after".as_slice(),
        ] {
            actions.extend(scanner.scan(chunk));
        }
        assert_eq!(
            actions,
            vec![
                InlineImageStreamAction::Bytes(b"before".to_vec()),
                InlineImageStreamAction::WorkingDirectory(b"file:///D:/a/b".to_vec()),
                InlineImageStreamAction::Bytes(b"mid".to_vec()),
                InlineImageStreamAction::WorkingDirectory(b"file:///D:/c".to_vec()),
                InlineImageStreamAction::Bytes(b"after".to_vec()),
            ]
        );

        // An empty report is the fact "no directory", and an oversized one is reported as the same
        // fact rather than as the prefix that happened to fit.
        let mut scanner = Osc1337Scanner::default();
        let oversized = format!("\x1b]7;file:///D:/{}\x07", "a".repeat(MAX_OSC_7_URI_BYTES));
        assert_eq!(
            scanner.scan(b"\x1b]7;\x07"),
            vec![InlineImageStreamAction::WorkingDirectory(Vec::new())]
        );
        assert_eq!(
            scanner.scan(oversized.as_bytes()),
            vec![InlineImageStreamAction::WorkingDirectory(Vec::new())]
        );

        // OSC 777 shares OSC 7's first byte and is a sequence of its own. Until §7.6 it was the
        // worked example of a prefix ruled out and emitted whole; now it is a notification, and
        // what this pins is that neither has taken the other's bytes — the `7;` above still
        // reports a directory and the `777;` here reports a message, in one scanner.
        let mut scanner = Osc1337Scanner::default();
        assert_eq!(
            scanner.scan(b"\x1b]777;notify;hi;there\x07x"),
            vec![
                InlineImageStreamAction::Notification(crate::session::TerminalNotification {
                    title: Some("hi".to_owned()),
                    body: "there".to_owned(),
                }),
                InlineImageStreamAction::Bytes(b"x".to_vec()),
            ]
        );
    }

    /// PIN (relative path ruling, 2026-08-03 (a)): the working directory rides the very same URI
    /// decoder the image peek uses — percent decoding, authority rules, per-segment joining — with
    /// the image extension allowlist deliberately not wired into it, because a directory has no
    /// extension to allow.
    #[test]
    fn working_directory_uris_decode_without_the_image_extension_gate() {
        for (uri, expected) in [
            (
                "file:///D:/Developer/BetterTerminal",
                r"D:\Developer\BetterTerminal",
            ),
            // A trailing slash is how a URI names a directory, not an empty final segment.
            ("file:///D:/Developer/", r"D:\Developer"),
            ("file:///D:/", r"D:\"),
            ("file:///D:/My%20Pictures", r"D:\My Pictures"),
            ("file:///D:/%E5%9B%BE%20%E7%89%87", r"D:\图 片"),
            ("file://localhost/D:/src", r"D:\src"),
            ("FILE:///D:/src", r"D:\src"),
        ] {
            assert_eq!(
                file_uri_to_local_path(uri, None),
                Some(PathBuf::from(expected)),
                "{uri:?}"
            );
        }
        // This machine's own name is the third spelling of "this host"; any other authority is a
        // remote share and names no directory this terminal may resolve against.
        assert_eq!(
            file_uri_to_local_path("file://MACHINE/D:/src", Some("machine")),
            Some(PathBuf::from(r"D:\src"))
        );
        for rejected in [
            "file://server/share/src",
            "file://MACHINE/D:/src",
            "file:///D://src",
            "file:///D:/a%zz",
            "file:///",
            "",
            "not a uri",
        ] {
            assert_eq!(file_uri_to_local_path(rejected, None), None, "{rejected:?}");
        }
        // The image peek keeps its own stricter reading: no hostname authority, and a trailing
        // slash names a directory, which is never an image.
        assert_eq!(
            file_uri_to_local_image_path("file://MACHINE/D:/a.png"),
            None
        );
        assert_eq!(file_uri_to_local_image_path("file:///D:/a.png/"), None);
    }

    /// A Windows shell may spell its directory the only way it can spell it.
    ///
    /// `cmd.exe`'s whole shell integration is the `PROMPT` variable, whose alphabet is a dozen
    /// `$`-substitutions and nothing else: `$P` expands to `D:\Developer`, and there is no
    /// operation in that language that could turn the separators into `/` or percent-encode the
    /// space in `C:\Program Files`. So the report arrives Win32-spelled, with raw spaces, and this
    /// decoder accepts it — a backslash is not a delimiter in a URI, so a Windows path survives
    /// `split('/')` as one segment, and `is_windows_drive_absolute` takes either separator.
    ///
    /// **Pinned because that acceptance is currently a consequence rather than a decision**, and
    /// the two are indistinguishable until someone tightens the parser. Rejecting a backslash, or
    /// requiring reserved characters to be encoded, are both reasonable-looking edits to a URI
    /// decoder; either one silently blanks the working directory of every Command Prompt pane, and
    /// nothing in this crate would notice, because from inside here it is one more malformed URI
    /// on the "forget rather than guess" path. The failure would surface as an empty `Recent` row
    /// and a pane head with no place in it, three layers away.
    ///
    /// The refusal underneath is the boundary of that leniency, and it is the only one in this
    /// family: leniency about *separators* buys nothing about *authority*, so a URI that names
    /// another host is still a remote share whichever way its slashes lean. Anything else that
    /// is not drive-rooted does not fail here at all — it leaves by the POSIX door
    /// (`Rooting::DriveOrPosixRoot`, pinned below), which is WSL's and is a different question.
    /// `cmd.exe` cannot reach that door in any case: `$P` is always drive-rooted, because `cmd`
    /// refuses to stand in a UNC directory at all.
    #[test]
    fn a_working_directory_may_be_spelled_the_way_a_windows_shell_can_spell_it() {
        for (uri, expected) in [
            // `$e]7;file:///$P$e\` at `D:\Developer\BetterTerminal`, measured off a real
            // pseudoconsole rather than composed here.
            (
                r"file:///D:\Developer\BetterTerminal",
                r"D:\Developer\BetterTerminal",
            ),
            // …and in a directory whose name has a space, which `PROMPT` cannot encode.
            (r"file:///C:\Program Files", r"C:\Program Files"),
            // The drive root, where `$P` is `C:\` and the separator is the one that makes it a
            // root rather than a name.
            (r"file:///C:\", r"C:\"),
            // Mixed, because nothing forbids it and a path is a path.
            (
                r"file:///D:\Developer/BetterTerminal",
                r"D:\Developer\BetterTerminal",
            ),
        ] {
            assert_eq!(
                file_uri_to_local_path(uri, None),
                Some(PathBuf::from(expected)),
                "{uri:?}"
            );
        }
        assert_eq!(
            file_uri_to_local_path(r"file://server\share\src", None),
            None,
            "a backslash in the path does not make another host's share ours"
        );
        // And a Windows-spelled path is still not an image reference: that reading is
        // `Rooting::DriveOnly` and stricter on purpose, but it is stricter about *rooting*, not
        // about separators, so it takes this one too.
        assert_eq!(
            file_uri_to_local_image_path(r"file:///D:\Developer\shot.png"),
            Some(PathBuf::from(r"D:\Developer\shot.png"))
        );
    }

    /// A shell that is not a Windows process still has a directory, and OSC 7 is the only way it
    /// can say so — the WSL half of `docs/shell-integration.md`'s working-directory contract.
    ///
    /// Red before `Rooting::DriveOrPosixRoot`: every one of these decoded to a *relative* path
    /// (`mnt\d\src`), failed `is_local_absolute_path`, and came back `None`, so a WSL pane could
    /// report its folder on every prompt and still be a pane that has never said where it is.
    #[test]
    fn a_working_directory_may_be_posix_rooted_while_an_image_reference_may_not() {
        for (uri, expected) in [
            ("file:///home/weiyi/src", "/home/weiyi/src"),
            (
                "file:///mnt/d/Developer/BetterTerminal",
                "/mnt/d/Developer/BetterTerminal",
            ),
            // The trailing-slash and percent-decoding rules are the decoder's, not the drive's.
            ("file:///mnt/d/Developer/", "/mnt/d/Developer"),
            ("file:///mnt/d/My%20Pictures", "/mnt/d/My Pictures"),
            ("file:///%E5%9B%BE%20%E7%89%87", "/图 片"),
        ] {
            assert_eq!(
                file_uri_to_local_path(uri, None),
                Some(PathBuf::from(expected)),
                "{uri:?}"
            );
        }
        // Every other gate still stands in front of it. A POSIX root buys no authority, no interior
        // empty segment and no broken escape.
        for rejected in [
            "file://server/home/weiyi",
            "file:///home//weiyi",
            "file:///home/%zz",
        ] {
            assert_eq!(file_uri_to_local_path(rejected, None), None, "{rejected:?}");
        }
        // **The image peek does not widen with it.** A reference is something this terminal opens,
        // and it opens through Windows; `/mnt/d/a.png` is a path only the shell that printed it can
        // resolve, so it is not a candidate here however plausible it looks.
        assert_eq!(file_uri_to_local_image_path("file:///mnt/d/a.png"), None);
    }

    /// PIN (relative path ruling, 2026-08-03, as widened the same day): the relative scan reads
    /// anchored (`./`, `../`) **and** bare references that carry a separator, keeps the absolute
    /// scan's boundary rules exactly, and reports the text as printed — resolution is a separate
    /// act that needs a directory this scan does not have.
    #[test]
    fn relative_candidates_are_anchored_or_bare_with_a_separator_and_report_their_printed_text() {
        let text = r#"see ./a.png and ..\b\c.svg and "./my pic.webp" and dir/d.png"#;
        let candidates = detect_relative_image_path_candidates(text);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.path.as_str())
                .collect::<Vec<_>>(),
            vec!["./a.png", r"..\b\c.svg", "./my pic.webp", "dir/d.png"]
        );
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.path != "./my pic.webp")
        {
            assert_eq!(
                &text[candidate.byte_start..candidate.byte_end],
                candidate.path,
                "an unquoted span addresses its own text exactly"
            );
        }
        // The two shapes the user reproduced, verbatim — plus the shapes the widening admits as a
        // consequence, which the anchored-only scan used to refuse for want of an anchor. A
        // directory really can be named `v1..`, and a name no filesystem holds costs one `stat`.
        for repro in [
            "local-images/sunset.svg",
            ".tmp-a85-parent/docs/spikes/artifacts/03-visual/c-fraction.svg",
            "v1../a.png",
            ".../a.png",
        ] {
            assert_eq!(
                detect_relative_image_path_candidates(&format!("生成了 {repro} 见图"))
                    .into_iter()
                    .map(|candidate| candidate.path)
                    .collect::<Vec<_>>(),
                vec![repro.to_owned()],
                "{repro:?} is a reference like any other"
            );
        }
        for rejected in [
            // The extension allowlist is the same one every shape meets.
            "./notes.txt",
            "../a.bmp",
            "dir/notes.txt",
            // A dot that continues a token opens nothing, which is what keeps the `./` inside a
            // URI and the `\.\` inside a native path out of this scan.
            "file:///D:/./a.png",
            r"D:\a\.\b.png",
            // A quoted candidate must close its quote (the unquoted re-read that follows stops at
            // the space, exactly as it does for an unterminated quoted absolute path).
            r#""./unterminated image.png"#,
        ] {
            assert!(
                detect_relative_image_path_candidates(rejected).is_empty(),
                "unexpected relative candidate in {rejected:?}"
            );
        }
        // The boundary generalization of 2026-08-02 applies unchanged: CJK prose and full-width
        // brackets open and close a relative candidate just as they do an absolute one — and a
        // bare reference in CJK prose is read from the colon that introduces it, never from the
        // prose in front of it.
        assert_eq!(
            detect_relative_image_path_candidates("见图（./图片.png）")[0].path,
            "./图片.png"
        );
        assert_eq!(
            detect_relative_image_path_candidates("路径：图片/日落.png")
                .into_iter()
                .map(|candidate| candidate.path)
                .collect::<Vec<_>>(),
            vec!["图片/日落.png".to_owned()],
            "one candidate, opened after the full-width colon and not at 路"
        );
        // A quoted reference keeps its spaces; a quoted region that is prose is re-read from
        // inside, so a reference sitting in it is still found.
        assert_eq!(
            detect_relative_image_path_candidates(r#""dir/my pic.webp""#)[0].path,
            "dir/my pic.webp"
        );
        assert_eq!(
            detect_relative_image_path_candidates(r#""see dir/a.png here""#)[0].path,
            "dir/a.png"
        );
        // A quote opens its own candidate wherever it stands, including hard against the word in
        // front of it — the shape every option assignment prints.
        for quoted_after_a_word in [
            r#"--input="./my pic.png""#,
            r#"src="dir/my pic.png""#,
            r#"abc"dir/my pic.png""#,
        ] {
            assert_eq!(
                detect_relative_image_path_candidates(quoted_after_a_word)
                    .into_iter()
                    .map(|candidate| candidate.path)
                    .collect::<Vec<_>>(),
                vec![quoted_after_a_word.split('"').nth(1).unwrap().to_owned()],
                "{quoted_after_a_word:?}"
            );
        }
    }

    /// PIN (relative path widening, 2026-08-03): the separator is the whole boundary between a
    /// bare reference and prose, and the character classes alone — never a URL sniff — keep a bare
    /// candidate from opening inside somebody else's text.
    ///
    /// RED CHECK: drop the `candidate.contains(['/', '\\'])` clause from
    /// `is_relative_image_reference` and every single-segment case below turns red at once.
    #[test]
    fn bare_relative_candidates_need_a_separator_and_never_open_inside_a_url() {
        for rejected in [
            // One word with a dot in it is prose. This is the ruling's ambiguity boundary.
            "readme.png",
            "x.png",
            "see readme.png here",
            "见 readme.png 图",
            "\"readme.png\"",
            // A scheme's authority is not a place on this disk, and the `//` it leaves behind
            // opens with a separator.
            "https://a.b/x.png",
            "see https://example.test/img/x.png here",
            "http://a.b/x.png",
            "file:///D:/x.png",
            "file://localhost/D:/x.png",
            // The tail behind a port colon is a well-formed relative name and is refused all the
            // same: a colon binds leftward.
            "https://host:8080/img/x.png",
            // Rooted, not relative: joining it to a working directory would invent a place.
            "/usr/share/pixmaps/x.png",
            r"\\server\share\x.png",
            r"\tmp\x.png",
            // Drive-rooted text is the native scan's, quoted or not.
            r"D:\dir\x.png",
            "D:/dir/x.png",
            r#""D:\dir\x.png""#,
        ] {
            assert!(
                detect_relative_image_path_candidates(rejected).is_empty(),
                "unexpected relative candidate in {rejected:?}"
            );
        }
        // The refusals are boundary decisions, not judgements about the text around them: the same
        // line that carries a URL still yields the bare reference printed beside it.
        let text = "see https://a.b/x.png and local-images/sunset.svg";
        assert_eq!(
            detect_relative_image_path_candidates(text)
                .into_iter()
                .map(|candidate| candidate.path)
                .collect::<Vec<_>>(),
            vec!["local-images/sunset.svg".to_owned()]
        );
    }

    /// PIN (relative path widening, 2026-08-03): the widened scan is linear in the line it reads.
    ///
    /// A bare reference has no prefix to be recognized by, so unlike the absolute scan this one
    /// considers an opening at nearly every character of a line carrying no whitespace and no
    /// closing delimiter — `,` and `:` are neither path characters nor token terminators, so the
    /// whole line is one token that opens a candidate every few characters. Such a line reaches
    /// this scan whole: a wrapped logical line is joined before it is read.
    ///
    /// Two things keep it linear, and dropping either one leaves this test running for hours: the
    /// token end is found once per token rather than once per opening, and the bare opening test —
    /// which stops at the first non-path character, and every opening has one in front of it, so
    /// no two openings read the same stretch — is asked before the tests that read the whole
    /// candidate.
    #[test]
    fn a_long_line_without_terminators_is_scanned_in_time_proportional_to_its_length() {
        let started = std::time::Instant::now();
        for line in [
            "a:1,b:2,c:3,".repeat(20_000),
            "dir/a,dir/b,dir/c,".repeat(20_000),
            "a.png,b.png,c.png,".repeat(20_000),
        ] {
            assert!(detect_relative_image_path_candidates(&line).is_empty());
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "three long lines took {:?}",
            started.elapsed()
        );
    }

    /// PIN (relative path widening, 2026-08-03): no text is claimed twice. A line carrying all
    /// three shapes at once yields exactly one candidate per printed reference, and their spans do
    /// not overlap — the widened bare scan reaches across the native scan's paths and the URI
    /// scan's URIs without touching either.
    #[test]
    fn overlapping_scans_never_claim_the_same_text_twice() {
        let cwd = PathBuf::from(r"D:\work");
        let text = "D:\\abs.png file:///D:/uri.png local-images/sunset.svg ./a.png \
                    https://a.b/x.png";
        let candidates = detect_peek_image_candidates(text, Some(&cwd));
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| (
                    &text[candidate.byte_start..candidate.byte_end],
                    candidate.path.as_str()
                ))
                .collect::<std::collections::BTreeSet<_>>(),
            [
                (r"D:\abs.png", r"D:\abs.png"),
                ("file:///D:/uri.png", r"D:\uri.png"),
                (
                    "local-images/sunset.svg",
                    r"D:\work\local-images\sunset.svg"
                ),
                ("./a.png", r"D:\work\a.png"),
            ]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
        );
        assert_eq!(candidates.len(), 4, "one candidate per printed reference");
        let mut spans = candidates
            .iter()
            .map(|candidate| (candidate.byte_start, candidate.byte_end))
            .collect::<Vec<_>>();
        spans.sort_unstable();
        assert!(
            spans.windows(2).all(|pair| pair[0].1 <= pair[1].0),
            "no two candidates overlap: {spans:?}"
        );
    }

    /// PIN (relative path ruling, 2026-08-03): resolution is a lexical join — no filesystem call,
    /// `..` popping the component to its left — followed by the same admission gate every other
    /// shape meets. A climb past the drive root names nothing.
    #[test]
    fn relative_candidates_resolve_lexically_against_a_working_directory() {
        let cwd = PathBuf::from(r"D:\a\b");
        for (relative, expected) in [
            ("./x.png", r"D:\a\b\x.png"),
            (r".\x.png", r"D:\a\b\x.png"),
            ("./sub/x.png", r"D:\a\b\sub\x.png"),
            ("../y.svg", r"D:\a\y.svg"),
            ("../../z.PNG", r"D:\z.PNG"),
            ("../b/./x.png", r"D:\a\b\x.png"),
        ] {
            assert_eq!(
                resolve_relative_image_path(&cwd, relative),
                Some(PathBuf::from(expected)),
                "{relative:?}"
            );
        }
        assert_eq!(
            resolve_relative_image_path(Path::new(r"D:\"), "./x.png"),
            Some(PathBuf::from(r"D:\x.png"))
        );
        for rejected in [
            // Above the drive root there is no path to name.
            "../../../x.png",
            // The extension allowlist, applied to the result exactly as to printed text.
            "./notes.txt",
        ] {
            assert_eq!(
                resolve_relative_image_path(&cwd, rejected),
                None,
                "{rejected:?}"
            );
        }
        // A directory that is not itself a drive-rooted local path is no authority at all.
        assert_eq!(
            resolve_relative_image_path(Path::new(r"\\server\share"), "./x.png"),
            None
        );
    }

    /// PIN (relative path ruling, 2026-08-03 (d), the honest-degradation pin): with no working
    /// directory, relative text yields no candidate through any union layer. The same text with a
    /// directory yields the resolved absolute path under the span of the relative text.
    #[test]
    fn relative_text_is_no_candidate_at_all_without_a_working_directory() {
        let text = "see ./a.png and D:\\abs.png and file:///D:/uri.png";
        let without = detect_peek_image_candidates(text, None);
        assert_eq!(
            without
                .iter()
                .map(|candidate| candidate.path.as_str())
                .collect::<Vec<_>>(),
            vec![r"D:\abs.png", r"D:\uri.png"],
            "no directory, no relative candidate — and every other shape is untouched"
        );
        assert!(
            detect_inline_image_candidates(text, None)
                .iter()
                .all(|candidate| candidate.path != "./a.png")
        );

        let cwd = PathBuf::from(r"D:\work");
        let with = detect_inline_image_candidates(text, Some(&cwd));
        assert_eq!(
            with.iter()
                .map(|candidate| candidate.path.as_str())
                .collect::<Vec<_>>(),
            vec![r"D:\abs.png", r"D:\work\a.png"],
            "inline admission reads the native path and the resolved relative one"
        );
        let relative = with
            .iter()
            .find(|candidate| candidate.path == r"D:\work\a.png")
            .unwrap();
        assert_eq!(
            &text[relative.byte_start..relative.byte_end],
            "./a.png",
            "the span covers the printed reference, the path is where it resolves"
        );
        assert_eq!(
            detect_peek_image_candidates(text, Some(&cwd)).len(),
            3,
            "the peek reads the line once through all three shapes"
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
                InlineImageStreamAction::ShellIntegration(
                    ShellIntegrationMarker::CommandFinished {
                        exit_code: Some(17),
                    },
                ),
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
        let stream = b"pre\x1b]7;file:///D:/w\x07\x1b]133;A\x07\x1b]133;B\x1b\\cmd\x1b]133;C\x07out\x1b]133;D;0\x1b\\post";
        type Normalized = (Vec<u8>, Vec<ShellIntegrationMarker>, Vec<Vec<u8>>);
        fn normalized(chunks: &[&[u8]]) -> Normalized {
            let mut scanner = Osc1337Scanner::default();
            let mut bytes = Vec::new();
            let mut markers = Vec::new();
            let mut directories = Vec::new();
            for chunk in chunks {
                for action in scanner.scan(chunk) {
                    match action {
                        InlineImageStreamAction::Bytes(part) => bytes.extend(part),
                        InlineImageStreamAction::ShellIntegration(marker) => markers.push(marker),
                        InlineImageStreamAction::WorkingDirectory(uri) => directories.push(uri),
                        InlineImageStreamAction::Image(_)
                        | InlineImageStreamAction::Progress(_)
                        | InlineImageStreamAction::Notification(_)
                        | InlineImageStreamAction::TooLarge => {
                            panic!("fixture contains no image")
                        }
                    }
                }
            }
            (bytes, markers, directories)
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
    /// PIN (user report 2026-08-18) — **the background picture is decoded
    /// against its own budget, not the inline image's.**
    ///
    /// The report was a 24 MP camera JPEG chosen from the Appearance page and
    /// refused with "inline image exceeds its decode limit". The gate that
    /// refused it was honest about an inline image and wrong about a wallpaper:
    /// one picture, chosen by hand, uploaded once.
    ///
    /// The probe is a 4200x4200 greyscale PNG — a few kilobytes of file and
    /// 70.6 MiB of RGBA, so it clears every file-size gate in the module and is
    /// stopped only by the pixel budget. The inline decoder refuses it; the
    /// background decoder takes it and hands back a texture cut to the ceiling.
    ///
    /// Red gate: point `decode_background_image` at
    /// `MAX_INLINE_IMAGE_RGBA_BYTES` and the second half goes red.
    #[test]
    fn a_background_picture_clears_the_pixel_budget_that_stops_an_inline_image() {
        const SIDE: u32 = 4200;
        assert!(
            u64::from(SIDE) * u64::from(SIDE) * 4 > MAX_INLINE_IMAGE_RGBA_BYTES,
            "the probe has to be past the inline budget or it proves nothing"
        );
        assert!(
            u64::from(SIDE) * u64::from(SIDE) * 4 < MAX_BACKGROUND_IMAGE_RGBA_BYTES,
            "and inside the background's, or it proves the wrong thing"
        );
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "betterterminal-background-budget-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("wallpaper.png");
        {
            // Greyscale on the way in, so the file this test writes is 17 MiB of
            // zeroes and not 70; the decode still expands to RGBA, which is the
            // side the budget is stated on.
            let plane: image::ImageBuffer<image::Luma<u8>, Vec<u8>> =
                image::ImageBuffer::new(SIDE, SIDE);
            plane.save_with_format(&path, ImageFormat::Png).unwrap();
        }

        let inline = decode_inline_image(InlineImageTask {
            occurrence_id: 0,
            source: InlineImageSource::LocalPath(path.clone()),
        });
        assert!(
            inline.is_err(),
            "an inline image of this size is still refused, which is the gate              the background was borrowing"
        );

        let ground = decode_background_image(&path, (3840, 2160)).unwrap();
        assert_eq!(
            (ground.width_px, ground.height_px),
            (2160, 2160),
            "and the background takes it, cut to the ceiling it was given"
        );
        assert!(
            ground.key.ends_with("@2160x2160"),
            "the resample target is part of the texture identity: {}",
            ground.key
        );
        assert_eq!(ground.rgba.len(), 2160 * 2160 * 4);

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    /// PIN — **the resample target is the largest monitor, one scale on both
    /// axes, and never an upscale.**
    #[test]
    fn a_background_picture_is_cut_to_the_ceiling_and_never_stretched_to_it() {
        // Smaller than the ceiling on both axes: untouched, because upsampling
        // on the CPU produces what the sampler would have produced anyway.
        assert_eq!(background_target_size((800, 600), (3840, 2160)), (800, 600));
        assert_eq!(
            background_target_size((3840, 2160), (3840, 2160)),
            (3840, 2160)
        );
        // Over on one axis: one scale, taken from the axis that is furthest
        // over, so the aspect ratio is the picture's own.
        assert_eq!(
            background_target_size((6000, 4000), (3840, 2160)),
            (3240, 2160)
        );
        assert_eq!(
            background_target_size((6000, 1000), (3840, 2160)),
            (3840, 640)
        );
        // A shape so extreme the short side rounds to nothing still has a pixel
        // on it: a zero-sided texture is not a smaller texture, it is no
        // texture.
        assert_eq!(
            background_target_size((100_000, 3), (3840, 2160)),
            (3840, 1)
        );
        // No ceiling to speak of is no resample: the caller could not name a
        // monitor, and inventing one here would be this module deciding how big
        // a screen is.
        assert_eq!(background_target_size((6000, 4000), (0, 0)), (6000, 4000));
    }

    /// PIN (user report 2026-08-18) — **a refused background picture says what
    /// happened to *this file*, in words a settings row can carry.**
    ///
    /// The reported sentence was "inline image exceeds its decode limit", which
    /// names a mechanism the reader of the Appearance page has never met and a
    /// limit that is not the one that was applied. Every sentence this type
    /// says is about the file the chooser just returned.
    #[test]
    fn a_refused_background_picture_never_speaks_of_inline_images() {
        for error in [
            BackgroundImageError::InvalidPath,
            BackgroundImageError::Io("access is denied".to_owned()),
            BackgroundImageError::TooLarge {
                bytes: 200 * 1024 * 1024,
                limit: MAX_BACKGROUND_IMAGE_BYTES,
            },
            BackgroundImageError::UnsupportedFormat,
            BackgroundImageError::Decode("truncated stream".to_owned()),
            BackgroundImageError::InvalidDimensions,
        ] {
            let sentence = error.to_string();
            assert!(
                !sentence.contains("inline"),
                "{error:?} leaks the inline vocabulary: {sentence}"
            );
            assert!(!sentence.is_empty());
        }
        assert_eq!(
            BackgroundImageError::TooLarge {
                bytes: 200 * 1024 * 1024,
                limit: MAX_BACKGROUND_IMAGE_BYTES,
            }
            .to_string(),
            "is 200 MB and a background picture may be up to 64 MB",
            "the sentence names both numbers, because \"too large\" alone              leaves a 70 MB file and a 700 MB one indistinguishable"
        );
    }
}
