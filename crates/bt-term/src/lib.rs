//! Folio's logic-only terminal actor and alacritty compatibility seam.

mod adapter;
mod cell_capture;
mod command_marks;
mod diagnostics;
mod inline_image;
mod lifecycle;
mod palette;
mod scheduling;
mod session;

pub use adapter::{
    AdapterEvent, MouseTracking, RemovalCause, RemovalContext, RemovalScope, RemovalScreen,
    RemovedLiveRow, SCROLLBACK_LINES, TerminalAdapter, TerminalCursor, TerminalDamage,
    TerminalModes,
};
pub use bt_detect::DetectionTask;
#[doc(hidden)]
pub use bt_doc::LayoutKey;
#[doc(hidden)]
pub use bt_math::MathEngine;
pub use command_marks::{CommandMark, CommandMarkId, CommandMarkLedger};
pub use diagnostics::{
    FormulaFlashOracle, FormulaFrameObservation, FormulaFrameState, band_owns_its_rows,
    is_banded_artifact, observe_formula_frame,
};
pub use inline_image::{
    BackgroundImageError, DecodedInlineImage, ImageReferenceShape, InlineImageDecodeError,
    InlineImageDecoder, InlineImageScaleTask, InlineImageSource, InlineImageTask,
    LocalImagePathCandidate, MAX_BACKGROUND_IMAGE_BYTES, MAX_BACKGROUND_IMAGE_RGBA_BYTES,
    MAX_INLINE_IMAGE_BYTES, ScaledInlineImage, ShellIntegrationMarker, background_target_size,
    decode_background_image, decode_inline_image, detect_inline_image_candidates,
    detect_local_image_path_candidates, detect_local_image_uri_candidates,
    detect_peek_image_candidates, detect_relative_image_path_candidates, display_texture_key,
    file_uri_to_local_image_path, file_uri_to_local_path, has_admissible_image_extension,
    local_host_name, mebibytes, normalized_local_image_path_key, resolve_relative_image_path,
    scale_inline_image,
};
pub use palette::{TerminalCanvas, TerminalPalette};

pub use lifecycle::{
    LIFECYCLE_RULES, LifecycleDirective, LifecycleRule, MatchValue, ResizePlan, RowAction,
    RowDirective, RowShape, classify, plan_resize,
};
pub use scheduling::{PARSE_QUANTUM, RESIZE_REQUEST_QUIET, WORKER_QUEUE_CAP};
pub use session::{
    DualPlaneSession, FrameImageReference, HeldUnbackedRecord, InlineImageRecordView,
    LIVE_MATH_READABLE_SCALE_MILLI, LIVE_MATH_STABLE_INTERVAL, LIVE_MIN_VISIBLE_TEXT_ROWS,
    MathLayoutOptions, ProgressState, ResizeTraceEvent, ResizeTraceKind, ResizeTraceRowOrigin,
    SPIKE_CELL_HEIGHT_SUBPIXELS, SessionDecorationTask, SessionError, SessionMathTask,
    SessionStatus, TerminalNotification, decoration_state_label, path_exists,
    render_detection_task, render_live_detection_task,
};
