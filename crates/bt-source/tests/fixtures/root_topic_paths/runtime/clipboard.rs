//! A topic. T11: a sibling's path from inside `runtime/` is accepted.

pub(crate) fn copy(n: u8) -> u8 {
    n + crate::runtime::pointer::press()
}

pub(crate) fn paste(n: u8) -> u8 {
    n
}

pub(crate) fn exported(n: u8) -> u8 {
    n
}
