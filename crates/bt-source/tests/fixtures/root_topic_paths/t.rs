//! T12 — a file reached by `#[cfg(test)] mod t;`: its paths are accepted.

use crate::runtime::clipboard::copy;

pub fn tested() -> u8 {
    crate::runtime::pointer::press() + copy(5)
}
