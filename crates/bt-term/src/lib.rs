//! Folio's logic-only terminal actor and alacritty compatibility seam.

#![cfg_attr(test, allow(clippy::disallowed_methods))]

mod adapter;
mod bounded_cache;
mod cell_capture;
mod command_marks;
mod diagnostics;
mod host;
mod inline_image;
mod lifecycle;
mod palette;
mod scheduling;
mod session;

pub use adapter::{
    AdapterEvent, KeyboardProtocol, ModifyOtherKeys, MouseTracking, PtyTransport, RemovalCause,
    RemovalContext, RemovalScope, RemovalScreen, RemovedLiveRow, RepaintControl, SCROLLBACK_LINES,
    SUPPORTED_KITTY_FLAGS, TerminalAdapter, TerminalCursor, TerminalDamage, TerminalModes,
};
pub use bounded_cache::{BoundedCache, Weighed};
pub use bt_detect::DetectionTask;
#[doc(hidden)]
pub use bt_doc::LayoutKey;
pub use command_marks::{CommandMark, CommandMarkId, CommandMarkLedger};
pub use diagnostics::{
    FormulaFlashOracle, FormulaFrameObservation, FormulaFrameState, band_owns_its_rows,
    is_banded_artifact, observe_formula_frame,
};
pub use host::{
    HOST_NAMES_READ_BEFORE_INSTALL, SVG_RASTERIZER_READ_BEFORE_INSTALL, SvgRasterizer,
    TEST_HOST_NAMES, TEST_SVG_DOCUMENT, TEST_SVG_PIXEL, install_host_names,
    install_pool_thread_start, install_svg_rasterizer, install_test_host_names,
    install_test_svg_rasterizer, local_host_names, test_svg_rasterizer,
};
pub use inline_image::{
    BackgroundImageError, DecodedInlineImage, ImageReferenceShape, InlineImageDecodeError,
    InlineImageDecoder, InlineImageScaleTask, InlineImageSource, InlineImageTask,
    LocalImagePathCandidate, MAX_BACKGROUND_IMAGE_BYTES, MAX_BACKGROUND_IMAGE_RGBA_BYTES,
    MAX_INLINE_IMAGE_BYTES, MAX_INLINE_IMAGE_RGBA_BYTES, MAX_LOCAL_IMAGE_FILE_BYTES,
    ScaledInlineImage, ShellIntegrationMarker, background_target_size, decode_background_image,
    decode_inline_image, detect_inline_image_candidates, detect_local_image_path_candidates,
    detect_local_image_uri_candidates, detect_peek_image_candidates,
    detect_relative_image_path_candidates, display_texture_key, file_uri_to_local_image_path,
    file_uri_to_local_path, has_admissible_image_extension, mebibytes,
    normalized_local_image_path_key, resolve_relative_image_path, scale_inline_image,
    size_within_rgba_budget,
};
pub use palette::{TerminalCanvas, TerminalPalette};

pub use lifecycle::{
    LIFECYCLE_RULES, LifecycleDirective, LifecycleRule, MatchValue, ResizePlan, RowAction,
    RowDirective, RowShape, classify, plan_resize,
};
pub use scheduling::{PARSE_QUANTUM, RESIZE_REQUEST_QUIET, WORKER_QUEUE_CAP};
pub use session::{
    AttentionRequest, BellSource, DualPlaneSession, FrameImageReference, HeldUnbackedRecord,
    HostScreen, InlineImageRecordView, LIVE_MATH_READABLE_SCALE_MILLI, LIVE_MATH_STABLE_INTERVAL,
    LIVE_MIN_VISIBLE_TEXT_ROWS, MathLayoutOptions, MathToggleFaces, MathTogglePresentation,
    NotificationSource, PathVerdict, ProgressState, ResizeTraceEvent, ResizeTraceKind,
    ResizeTraceRowOrigin, SPIKE_CELL_HEIGHT_SUBPIXELS, SessionDecorationTask, SessionError,
    SessionMathTask, SessionStatus, TerminalNotification, decoration_state_label,
    extend_live_task_band, live_snapshot_logical_line_text, path_exists, verify_path,
};

/// **The adapter seam imports no policy crate** (the layering Task 00b
/// repaired).
///
/// `adapter` and `cell_capture` are the vendor compatibility seam: their job is
/// to report terminal facts. Importing document, detection or viewport policy
/// there recreates the layering bug Task 00b removed — and the compiler allows
/// it, because this crate depends on all three for its session. This is what
/// does not.
///
/// The two modules are named by their paths, each with its whole tree, so a
/// submodule either one gains is covered the day it is declared. Every byte of
/// them is read, comments included, with a name's boundary on both sides: a doc
/// link that names a policy crate is the import the next edit writes.
///
/// MUTATION: append `use bt_doc::HistoryDocument as _AdapterBoundaryProbe;` to
/// `adapter.rs` and this goes red naming the line.
#[cfg(test)]
mod adapter_boundary_tests {
    use std::path::PathBuf;

    use bt_source::{
        DiskScope, Index, ModuleSpec, Pattern, Scope, Search, TargetId, TargetKind, TargetRoot,
        Universe, Vendor, View, needle, report,
    };

    const ALTERNATE_SOURCE_ROOT: &str = "BT_ADAPTER_BOUNDARY_SOURCE_ROOT";

    fn alternate_index() -> Option<Index> {
        let root = std::env::var_os(ALTERNATE_SOURCE_ROOT)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)?;
        let universe = Universe::declare(
            format!("bt-term's adapter seam at {}", root.display()),
            vec![TargetRoot {
                id: TargetId {
                    package: "bt-term".to_owned(),
                    kind: TargetKind::Library,
                    name: "bt-term".to_owned(),
                },
                file: root.join("lib.rs"),
            }],
            vec![DiskScope::under(root)],
            Vendor::Excluded,
        )
        .unwrap_or_else(|rejection| panic!("the alternate bt-term source root: {rejection}"));
        Some(Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections))))
    }

    #[test]
    fn the_adapter_seam_imports_no_policy_crate() {
        let alternate = alternate_index();
        let index = alternate
            .as_ref()
            .unwrap_or_else(|| Index::of_package("bt-term"));
        let mut found = Vec::new();
        for policy in ["bt_doc", "bt_detect", "bt_viewport"] {
            let named = index
                .search(
                    &Search::new(needle!(Pattern::identifier(policy)), View::Raw).in_scope(
                        Scope::Modules(vec![
                            ModuleSpec::tree("crate::adapter"),
                            ModuleSpec::tree("crate::cell_capture"),
                        ]),
                    ),
                )
                .unwrap_or_else(|failure| panic!("{failure}"));
            for occurrence in named.occurrences() {
                let location = index
                    .locate(occurrence.span.start())
                    .expect("an occurrence is in a file of the index");
                found.push(format!("{location}: {policy}"));
            }
        }
        assert!(
            found.is_empty(),
            "the adapter seam names a policy crate: {found:#?}"
        );
    }
}
