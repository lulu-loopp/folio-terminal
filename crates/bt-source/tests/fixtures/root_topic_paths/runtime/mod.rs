//! The fixture's `runtime`: two reachable topics, one private, an item re-export (accepted) and a
//! module re-export (F4, refused).

pub(crate) mod clipboard;
pub(crate) mod pointer;
mod quiet;

pub(crate) use clipboard::exported;
// F4 — PLANTED: a re-export of a module, which opens it under a new name: refused.
pub(crate) use pointer::inner;
