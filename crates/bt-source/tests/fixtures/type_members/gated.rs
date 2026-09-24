//! One type, two arms — and a field only one of the arms carries.
//!
//! This is the shape §2.4's eleven have, one lane over: the reading is
//! `cfg`-blind, so both declarations are in the index and a query that found the
//! field in one of them would be answering about one platform while saying
//! something about the type.

#[cfg(windows)]
pub struct Split {
    pub shared: u8,
    pub only_on_windows: u8,
}

#[cfg(not(windows))]
pub struct Split {
    pub shared: u8,
}
