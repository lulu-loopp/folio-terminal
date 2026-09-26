//! **The first window's GPU, asked for and waited on: an owner-thread door** (§5.3 row 23,
//! `doors::GpuOpen`; design note `docs/plans/design/thread-door-2026-09-26.md`, revision (e)2).
//!
//! `Runtime::create` opens the device the whole process draws with, and the surface that chose
//! its adapter, before the first window can present. It blocks the window thread on
//! `pollster::block_on` for as long as the adapter and the device take to answer. Row 23 records
//! that wait as found and not ruled (`DESIGN.md`, 2026-09-26); it stays on this thread until
//! device recovery rebuilds on a worker (B9, D-77). Until then it is admitted like every other
//! owner-thread wait: on the window thread, in `Running`, measured by the meter.

use bt_platform::admission::{WaitToken, doors};
use bt_render::{GpuContext, RenderError, WindowRenderer, WindowTarget};

/// **Open the process's device and the first window's surface on it.** The one `pollster`
/// wait on the window thread's launch road, admitted by its token.
pub(crate) fn open_first_window(
    token: WaitToken<'_, doors::GpuOpen>,
    target: WindowTarget,
    width: u32,
    height: u32,
    scale: f64,
) -> Result<(GpuContext, WindowRenderer), RenderError> {
    let _ = token;
    pollster::block_on(GpuContext::open(target, width, height, scale))
}
