//! A topic.

pub struct Press;

pub(crate) fn press() -> u8 {
    super::quiet::hush()
}
