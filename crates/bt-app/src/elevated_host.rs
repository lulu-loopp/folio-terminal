//! **The elevated host's door** — `folio --elevated-host <wire-version>
//! <pipe-name> <parent-pid> <parent-start-id> <capability>`, the exact private
//! line `bt_platform::elevated_pipe::launch_elevated_host` starts through
//! `runas` (design note `docs/plans/design/admin-terminal-2026-10-04.md` §2).
//!
//! Checked second in `main`, before console adoption and before anything a
//! resident does: this process takes no instance claim, opens no launch or
//! attention endpoint, reads no settings and writes no diagnostics. It opens
//! the pipe its line names, authenticates both ways within the attempt's
//! deadline, and exits; the relay that keeps it running is T-ADMIN-4's.

use std::time::Instant;

use bt_platform::elevated_protocol::{HostLine, LAUNCH_TIMEOUT};

/// The exit code of a host that authenticated.
pub const AUTHENTICATED: i32 = 0;
/// The exit code of a host whose handshake failed.
pub const REFUSED: i32 = 1;
/// The exit code of a line that is not the host's exact grammar.
pub const USAGE: i32 = 2;

/// Run the host door for one parsed line and answer its exit code.
pub fn serve(line: &HostLine) -> i32 {
    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    let handshake = bt_platform::admission::enter_standalone_main("folio-elevated-host", |_| {
        bt_platform::elevated_pipe::connect_to_parent(line, deadline)
    });
    match handshake {
        Ok(Ok(_authenticated)) => AUTHENTICATED,
        Ok(Err(_)) | Err(_) => REFUSED,
    }
}
