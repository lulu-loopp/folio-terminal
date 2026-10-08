//! **The effect vocabulary every layer shares.**
//!
//! Two modules, each the one owner of its facts, with no platform `cfg` and no
//! first-party dependency:
//!
//! - [`admission`] — which kind of thread this is ([`admission::Role`]), where the
//!   window thread is in its run ([`admission::Phase`]), the door registry as types
//!   ([`admission::doors`]), and the admission that lets the window thread wait at
//!   one of those doors ([`admission::admitted`], [`admission::WaitToken`]);
//! - [`file_reads`] — the process-wide ledger of file content read, by lane.
//!
//! **What is not here.** The thread door (`bt_platform::spawn_at_priority`) sets a
//! thread's scheduling band, which is the platform's, so it stays in `bt-platform`
//! and calls [`admission::lend_worker`] on the thread it starts. `bt-platform`
//! re-exports both modules as `bt_platform::admission` and `bt_platform::file_reads`,
//! so every path through it keeps resolving.
//!
//! This crate starts no thread, waits on nothing, reads no clock of the standard
//! library and names no platform crate; the only file system calls are the ledger's
//! own opens in [`file_reads`]. `bt-app`'s window-waits guard holds it to that.

#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod admission;
pub mod file_reads;
