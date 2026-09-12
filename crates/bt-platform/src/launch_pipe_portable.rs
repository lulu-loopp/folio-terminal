//! **The second launch's door into the first, before it is a Unix socket**
//! (M3-5).
//!
//! `docs/DESIGN.md` §7.59's semantics — a `Decision`, an `Admission`, and the
//! client's `CONFIRM` as the commit point — are the product's and travel
//! unchanged; the transport does not. M3-5 owns the port and it is a bigger
//! ticket than it looks, because §4.4 ④ of the plan found that the *claim* this
//! channel hangs off always succeeds off Windows: `instance::claim_data_directory`
//! answers `Some` on a machine with no kernel object to ask, so the
//! single-writer guarantee is **absent by construction** and M3-5 builds it
//! rather than preserving it. Socket-path length limits, peer verification
//! (DESIGN §7.59b), stale-endpoint cleanup and crash recovery come with it.
//!
//! Until then a second launch opens a second window, which is what every Folio
//! did before this channel existed and is exactly the fallback the caller
//! already takes when the endpoint is not there.

use std::io;
use std::path::Path;
use std::time::Duration;

/// How long a handover may take before the client gives up and opens its own
/// window. Wire policy, unchanged.
pub const HANDOVER_BUDGET: Duration = Duration::from_secs(2);

/// The one word the client sends to commit. Wire policy, unchanged.
pub const CONFIRM: &str = "ok";

/// **What the first process decided about the second's request.**
///
/// The semantics §7.59 fixes, and they are the product's rather than the
/// transport's: `admitted` is `Some` only for a request that was let in, and it
/// is dropped un-committed on every path where the client does not confirm —
/// which is what makes a reservation held inside it safe.
pub struct Decision<T> {
    /// The one line written back to the client.
    pub reply: String,
    /// The launch itself, if this decision admitted one.
    pub admitted: Option<T>,
}

/// **The endpoint the first launch listens on.**
///
/// Refused; M3-5. The caller's own failure path is "this process opens the
/// window itself", which is correct behaviour here rather than a degradation:
/// with no single-writer claim to build on, a handover would be handing a
/// window to a process that has no better title to the data directory than the
/// one handing it over.
pub struct LaunchPipe {
    /// Never constructed: [`LaunchPipe::start`] refuses.
    _never: std::convert::Infallible,
}

impl LaunchPipe {
    /// Open the endpoint. Refused; M3-5.
    pub fn start<T, D, C>(directory: &Path, decide: D, commit: C) -> io::Result<Self>
    where
        T: Send + 'static,
        D: Fn(&str) -> Option<Decision<T>> + Send + 'static,
        C: Fn(T) + Send + 'static,
    {
        let _ = (directory, decide, commit);
        Err(unsupported())
    }

    /// Unreachable: there is no value of this type.
    #[must_use]
    pub fn name(&self) -> &str {
        match self._never {}
    }
}

/// The name a launch endpoint for this data directory would have.
///
/// `None`, because a name nobody can listen on is worse than no name: the
/// caller reads `None` as "there is no channel here" and opens its own window,
/// and a name would have it try, fail and take the handover budget to do it.
#[must_use]
pub fn endpoint_for(directory: &Path) -> Option<String> {
    let _ = directory;
    None
}

/// **The client half** — hand this launch's command line to the process that
/// already owns the data directory.
///
/// Refused; M3-5. `NotFound` would be the answer for a name nobody is on, and
/// this is the stronger statement: there is no channel on this platform at all.
pub fn hand_over(
    endpoint: &str,
    request: &str,
    on_reply: impl FnOnce(u32, &str),
) -> io::Result<()> {
    let _ = (endpoint, request, on_reply);
    Err(unsupported())
}

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "the launch endpoint is not on this platform yet",
    )
}
