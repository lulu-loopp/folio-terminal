//! **Why a first frame was not drawn** — one owner for every `video` arm
//! (T-FIRST-FRAME-RUN-OUT-IS-NOT-A-REFUSAL), each of which re-exports it as
//! `video::NoFrame`.
//!
//! The arms write their own [`VideoFrame`](crate::video::VideoFrame) for the
//! reason `video_portable.rs`'s module note gives; this type is a rule rather
//! than a platform, so it is written once and the three arms cannot disagree
//! about it.

/// **The two ways a first frame is not drawn, which a caller must tell apart.**
///
/// They used to be one silence, `None`, and the one caller filed both as
/// "no decoder for this container". A refusal is a fact about the file: the
/// same question will be refused again. A run-out is a fact about the moment:
/// the question was not answered inside the budget — usually because the
/// machine was busy — and says nothing about the file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoFrame {
    /// Asked and answered with no picture: not a video, a container or codec
    /// this machine has no decoder for, a truncated file, a file that is gone,
    /// or a decode that panicked (the same file would panic it again).
    Refused,
    /// Not answered: the budget ran out before the decode did, or the thread
    /// that would have asked could not be started. The same question may be
    /// answered later.
    RanOut,
}
