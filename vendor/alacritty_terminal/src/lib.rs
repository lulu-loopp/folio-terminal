// MODIFIED BY THE FOLIO CONTRIBUTORS — not the upstream
// alacritty_terminal 0.26.0 file of the same name.
// Change: `event_loop` and `tty` are not compiled for `wasm32`, which has no
// pseudoterminal and no `polling` backend.
// Index: vendor/alacritty_terminal/CHANGES-FOLIO.md
// Notice given under section 4(b) of the Apache License, Version 2.0.

//! Alacritty - The GPU Enhanced Terminal.

#![warn(rust_2018_idioms, future_incompatible)]
#![deny(clippy::all, clippy::if_not_else, clippy::enum_glob_use)]
#![cfg_attr(clippy, deny(warnings))]

pub mod event;
#[cfg(not(target_arch = "wasm32"))]
pub mod event_loop;
pub mod grid;
pub mod index;
pub mod selection;
pub mod sync;
pub mod term;
pub mod thread;
#[cfg(not(target_arch = "wasm32"))]
pub mod tty;
pub mod vi_mode;

pub use crate::grid::Grid;
pub use crate::term::Term;
pub use vte;
