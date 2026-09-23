//! **`Runtime`'s inherent methods, by topic** (`docs/plans/bt-app-split.md` §6.1).
//!
//! One file per topic, each holding one `impl Runtime<'_>` block and the
//! methods of that topic byte for byte as `main.rs` wrote them.
//!
//! **This file declares modules and imports nothing.** Twelve of the topic
//! stems are also the name of a crate-root module, and `mod settings;` beside
//! `use crate::settings;` is `error[E0255]`. A topic file writes its own
//! imports, where the two names cannot collide.

mod attention;
mod clipboard;
mod diagnostics;
mod dpi;
mod first_run;
mod frame;
mod i18n;
mod palette;
mod profiles;
mod quake;
mod search;
mod terminal;
mod tooltips;
