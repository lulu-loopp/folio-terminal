//! BetterTerminal's logic-only terminal actor and alacritty compatibility seam.

mod adapter;
mod cell_capture;
mod lifecycle;
mod scheduling;
mod session;

pub use adapter::{
    AdapterEvent, MouseTracking, RemovalCause, RemovalContext, RemovalScope, RemovalScreen,
    RemovedLiveRow, SCROLLBACK_LINES, TerminalAdapter, TerminalCursor, TerminalModes,
};
pub use bt_detect::DetectionTask;
pub use lifecycle::{
    LIFECYCLE_RULES, LifecycleDirective, LifecycleRule, MatchValue, ResizePlan, RowAction,
    RowDirective, RowShape, classify, plan_resize,
};
pub use scheduling::{PARSE_QUANTUM, RESIZE_REQUEST_QUIET, WORKER_QUEUE_CAP};
pub use session::{
    DualPlaneSession, ResizeTraceEvent, ResizeTraceKind, ResizeTraceRowOrigin,
    SPIKE_CELL_HEIGHT_SUBPIXELS, SessionError, render_detection_task,
};
