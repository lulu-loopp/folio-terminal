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
mod configuration;
mod diagnostics;
mod dpi;
mod files;
mod first_run;
mod floats;
mod frame;
mod git;
mod handoff;
mod i18n;
mod keyboard;
mod math;
mod mouse;
mod palette;
mod panes;
mod peek;
pub(crate) mod pointer;
mod preview;
mod profiles;
mod quake;
mod search;
mod tabs;
pub(crate) use tabs::owe_birth_death_fallback_resize;
pub(crate) use windows::AfterTheWait;
#[cfg(test)]
pub(crate) use windows::recent_term_leaf_seed;
// K2 (T-SPLIT-2B): the free functions moved out of `main.rs`, named at the crate root again by
// its `use runtime::{…}` lines so that no caller changes.
#[cfg(test)]
pub(crate) use diagnostics::resolved_theme_change;
#[cfg(test)]
pub(crate) use pointer::a_capture_still_holds_its_pointer;
pub(crate) use pointer::platform_pointer_of;
#[cfg(test)]
pub(crate) use profiles::profile_menu_anchor;
#[cfg(test)]
pub(crate) use search::rescan_leaf_for_search;
#[cfg(test)]
pub(crate) use tooltips::{tab_surface_tip_boxes, tooltip_anchor_for};
mod terminal;
mod tooltips;
mod update_card;
mod web;
mod windows;
