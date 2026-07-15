//! BetterTerminal's logic-only terminal actor and alacritty compatibility seam.

mod adapter;
mod cell_capture;
mod lifecycle;
mod scheduling;
mod session;

pub use adapter::{AdapterEvent, RemovedLiveRow, SCROLLBACK_LINES, TerminalAdapter};
pub use lifecycle::{LIFECYCLE_RULES, LifecycleDirective, LifecycleEventKind, LifecycleRule};
pub use scheduling::{PARSE_QUANTUM, WORKER_QUEUE_CAP};
pub use session::{DualPlaneSession, SPIKE_CELL_HEIGHT_SUBPIXELS, SessionError};
