//! A topic.

pub struct Press;

pub(crate) fn press() -> u8 {
    super::quiet::hush()
}

/// A module inside a topic, which `runtime/mod.rs` re-exports (F4).
pub(crate) mod inner {
    pub struct Deep;
}
