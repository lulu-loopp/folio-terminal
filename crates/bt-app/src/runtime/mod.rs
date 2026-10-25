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
pub(crate) use attention::{
    answer_attention_in, merge_notice_card, next_attention_stop, raise_cancelled_quit_card_on,
};
#[cfg(test)]
pub(crate) use clipboard::{
    clear_copied_selection, clipboard_focus_of, glass_allows_a_text_write,
    input_line_needs_a_space_first, paste_answer_text, paste_body, paste_card_step,
    paste_offer_is_kept, paste_target_is_live, paste_text, pending_paste_in,
    prepare_clipboard_paste, prepare_dropped_paste, selection_to_copy, stage_paste,
    take_pending_paste,
};
#[cfg(test)]
pub(crate) use diagnostics::resolved_theme_change;
#[cfg(test)]
pub(crate) use dpi::{
    apply_tabs_leaf_metrics, resize_worth_solving, take_psreadline_resize_reanchor_input,
};
#[cfg(test)]
pub(crate) use floats::{
    chevron_turn_target, file_menu_powers, files_float_content_height, float_viewport_rect,
    refused_preview_card,
};
#[cfg(test)]
pub(crate) use git::{
    float_git_page_shown, git_answer_notice, git_full_path, git_roots_on_glass,
    git_surfaces_the_kernel_moved, git_surfaces_wanting_reread, graph_key_of,
};
#[cfg(test)]
pub(crate) use palette::recall_flash;
#[cfg(test)]
pub(crate) use peek::{
    ask_for_peek_pixels, file_peek_promotion, let_go_unnamed_peek_records, peek_body_kind,
    peek_cache_key_for_decode, peek_card_opened, peek_files_named, peek_foot_press,
    peek_page_texture_key, peek_reads_the_file, peek_scale_task, preview_body_bar, remember_decode,
};
#[cfg(test)]
pub(crate) use pointer::a_capture_still_holds_its_pointer;
pub(crate) use pointer::platform_pointer_of;
#[cfg(test)]
pub(crate) use profiles::profile_menu_anchor;
#[cfg(test)]
pub(crate) use search::rescan_leaf_for_search;
#[cfg(test)]
pub(crate) use terminal::{
    apply_leaf_metrics, drain_may_take_another_slice, in_drain_feed_turn, local_image_activation,
    reference_card, reference_run_rect, restart_seed, seat_holds_the_keyboard,
    should_copy_on_select_release, step_leaf_text_scale, stepped_command_mark,
    terminal_link_answers_a_press,
};
#[cfg(test)]
pub(crate) use tooltips::{tab_surface_tip_boxes, tooltip_anchor_for};
#[cfg(test)]
pub(crate) use web::{
    a_page_is_off_the_glass, a_page_still_has_a_pane, a_page_was_replaced,
    a_retirement_happens_on_this_turn, hole_for, revived_page_of,
};
mod terminal;
mod tooltips;
mod update_card;
mod web;
mod windows;
