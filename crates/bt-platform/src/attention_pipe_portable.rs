//! **The attention endpoint on a platform with neither a pipe namespace nor a
//! socket to bind** (M4-7).
//!
//! A third arm rather than a second, on [`crate::http`]'s footing (§13.27) and
//! `launch_pipe_portable.rs`'s: the two real ones are two transports — a named
//! pipe with a descriptor of our own, and a Unix socket in a private runtime
//! directory — and this is neither, which is what it says honestly rather than
//! quietly.
//!
//! The change of meaning between the two real arms is stated where both of them
//! can be read: the Windows descriptor names the **logon session**,
//! deliberately, "so a second session of the same user (a service, another
//! desktop) is outside it", and Unix owner permissions identify a **user**. See
//! `attention_pipe_unix.rs`'s header and `docs/DESIGN.md` §13.37, which write
//! that down as a decision rather than substituting `0600` and calling the two
//! equivalent.
//!
//! Here the channel is absent and says so. The three doors `bt-app` writes are
//! the server (`AttentionPipe::start`), the client (`send_line`) and the nonce
//! (`unguessable_bits`); the first two refuse and the third is real, because a
//! random number is not a platform question.

use std::io;

/// The longest line the endpoint will carry, unchanged: this is a bound the
/// product chose, not one the transport imposes.
pub const MAX_MESSAGE_BYTES: usize = 4096;

/// The most frames one second may carry, unchanged, for the same reason.
pub const MAX_FRAMES_PER_SECOND: u32 = 512;

/// What the endpoint has seen. All zero, because it has seen nothing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PipeCounts {
    /// Frames handed to the caller's sink.
    pub delivered: u64,
    /// Frames longer than [`MAX_MESSAGE_BYTES`].
    pub oversize: u64,
    /// Frames refused because this second's allowance was spent.
    pub throttled: u64,
    /// Connections that closed without saying anything.
    pub silent: u64,
    /// Clients that attached — the conservation law: every one of them becomes
    /// exactly one of the four counts above.
    pub accepted: u64,
}

/// **The endpoint a background agent rings.**
///
/// Refused, and the refusal is the honest one: an endpoint that started and
/// never delivered anything would make `folio attention` succeed silently and
/// the reader would wait for a notification that no channel could carry.
/// `bt-app`'s caller logs the failure once and the attention block reports
/// itself unavailable, which is what M4-7 turns back on.
pub struct AttentionPipe {
    /// Never constructed: [`AttentionPipe::start`] refuses.
    _never: std::convert::Infallible,
}

impl AttentionPipe {
    /// Open the endpoint for `directory`. Refused, because this platform has
    /// neither a pipe namespace nor a socket to put one in.
    pub fn start(
        directory: &std::path::Path,
        deliver: impl Fn(String) + Send + 'static,
    ) -> io::Result<Self> {
        let _ = (directory, deliver);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the attention endpoint is not on this platform yet",
        ))
    }

    /// Unreachable: there is no value of this type.
    #[must_use]
    pub fn name(&self) -> &str {
        match self._never {}
    }

    /// Unreachable.
    #[must_use]
    pub fn counts(&self) -> PipeCounts {
        match self._never {}
    }
}

/// **The client half** — one line into somebody else's endpoint.
///
/// Refused, with the error `bt_app::attention_wire` already reads as "nobody is
/// listening": the verb prints that and exits non-zero, which is what a hook
/// running against a Folio that cannot carry attention should see.
pub fn send_line(endpoint: &str, line: &str) -> io::Result<()> {
    let _ = (endpoint, line);
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the attention endpoint is not on this platform yet",
    ))
}

/// **Bits nobody can guess**, and this one is real on every platform.
///
/// The name in the endpoint carries a nonce so that a second process cannot
/// compute the first one's channel. That is arithmetic and entropy rather than
/// a platform question, and `getrandom` is in the standard library's own
/// hasher: `RandomState` seeds itself from the operating system, and two of its
/// hashes of a fixed key are a hundred and twenty-eight bits nobody can predict
/// without the seed.
///
/// **Not a cryptographic generator**, and it does not need to be: what the
/// nonce buys is that another process of the same user cannot *guess* the
/// endpoint's name, and the security boundary itself is the socket's own
/// permissions — which is M4-7's subject and R6's warning.
#[must_use]
pub fn unguessable_bits() -> u128 {
    use std::hash::{BuildHasher, Hasher};

    let mut high = std::collections::hash_map::RandomState::new().build_hasher();
    high.write_u8(0);
    let mut low = std::collections::hash_map::RandomState::new().build_hasher();
    low.write_u8(1);
    (u128::from(high.finish()) << 64) | u128::from(low.finish())
}
