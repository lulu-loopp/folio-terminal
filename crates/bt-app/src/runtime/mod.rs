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
pub(crate) use windows::{
    answer_an_exit_over, broker_verdict, dirty_gate_names, dwm_dark_mode_owed, focus_leaf_index,
    lines_phrase, persisted_window_bounds, preview_source_of_recent, raise_dirty_gate_over,
    recent_term_leaf_seed, recorded_window_placement, tear_out_rect, window_minimum_changed,
};
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
    take_pending_paste, text_field_holding,
};
#[cfg(test)]
pub(crate) use diagnostics::resolved_theme_change;
#[cfg(test)]
pub(crate) use dpi::{
    apply_tabs_leaf_metrics, resize_worth_solving, take_psreadline_resize_reanchor_input,
};
#[cfg(test)]
pub(crate) use files::{
    column_keyboard, columns_wanting_git, dropped_files_seat_at, files_key_under, files_key_within,
    files_keyboard_seat_of, files_locate, files_open_chain, files_root_of, files_row_activation,
    files_row_counts_clicks, files_row_entry, files_row_menu_subject, folder_foot_notice,
    folder_pick_outcome, press_files_node, reroot_files_state, switcher_pin_is_allowed,
};
#[cfg(test)]
pub(crate) use floats::{
    chevron_turn_target, file_menu_powers, files_float_content_height, float_viewport_rect,
    refused_preview_card,
};
#[cfg(test)]
pub(crate) use frame::{
    a_bare_redraw_still_owes_a_present, advance_periodic_deadline, attention_ledger_deadline,
    chrome_over, chrome_tick_reuses_picture, dispatch_decoration_task, dispatch_pending_math_tasks,
    dispatch_tab_decoration_tasks, earliest_named_deadline, math_worker_stopped_notice,
    pty_drain_says_nothing_new, pty_frame_is_unchanged, startup_poll_delay,
    take_math_worker_notice, verified_target_of, window_is_exposed, window_is_hidden,
};
#[cfg(test)]
pub(crate) use git::{
    float_git_page_shown, git_answer_notice, git_full_path, git_roots_on_glass,
    git_surfaces_the_kernel_moved, git_surfaces_wanting_reread, graph_key_of,
};
#[cfg(test)]
pub(crate) use keyboard::{
    composing_event_of, composition_outlived_its_field, composition_ruling, goto_tab_index,
    ime_caret_source, ime_commit_bytes, ime_cursor_area_of, keyboard_owner_is_a_shell,
    paste_card_key, preedit_caret_byte, rename_key,
};
#[cfg(test)]
pub(crate) use math::{
    answer_one_formula, answers_for, claimed_by, document_formulas, ground_overlay_layers,
    hold_failed_in_tabs, math_copy_window, new_tab_tip, retire_spent_math_copy,
};
#[cfg(test)]
pub(crate) use mouse::{
    a_right_press_is_on_the_pane_menus_head, answered_once, button_router_position,
    drain_whole_units, float_grasp, glance_may_arm, glass_allows_a_drop, landing_for_aim,
    link_target_to_ask_about, live_viewport_mouse_hit, over_home_ground, pointer_cursor,
    press_after_blur, press_belongs_to_the_window, press_pins_a_peek, press_reaches_no_grid,
    press_spends_itself_closing, pressed_row_identity, recoverable_wheel_scroll_amount,
    release_verdict, right_press_raises_terminal_menu, route_forwarded_mouse_button,
    route_forwarded_mouse_motion, upright_wheel, wheel_axis, wheel_points_sideways, wheel_route,
    wheel_steps_text_size,
};
#[cfg(test)]
pub(crate) use palette::recall_flash;
#[cfg(test)]
pub(crate) use panes::{
    animated_pane_viewports, births_due, clamp_into_body, column_notch, end_leaves_of,
    frame_row_of_anchor, gate_title_over, host_screen_after, land_successor, landed_source,
    leaf_resize_plan, pages_that_move_with_their_panes, pane_fade_veil_layers, panel_opacity,
    popup_owner, rail_change_strands_its_popups, rail_is_searching, rail_overlay_layer,
    rail_zone_wants_open, restated_scroll, row_arrival_seat, size_authority_for_rectangle,
    tear_pane_into_tab,
};
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
pub(crate) use preview::{
    animation_layer_key, animation_refusal_notice, animation_surface_exists, answer_one_picture,
    ask_to_open, build_preview_diff_body, build_preview_markdown_body, build_preview_table_body,
    build_preview_text_body, crumb_segments, crumb_segments_on, deliver_clipboard_picture,
    documents_held_in, documents_held_mut_in, file_peek_answer, folded_levels, foot_revealed_label,
    forget_a_picture, forget_standing_answers, image_clamped_pan, image_destination,
    image_is_pannable, image_meta_sentence, image_pan_key, image_raster_cap, image_zoom_caption,
    image_zoom_key, image_zoom_notch, image_zoom_settles, image_zoom_toggled, markdown_chip_ground,
    markdown_empty_page_offset, markdown_prose_paragraphs, markdown_source_cell,
    markdown_source_offset_at, owe_sharpened_rasters, page_address_hand_off, page_foot_flash,
    page_foot_lead, page_source_file, peek_awaits, picture_errand, picture_files_of,
    pictures_awaited_by, pictures_need_handing_over, preview_block_bar_at, preview_block_wheel,
    preview_caret_row, preview_copies_on_select, preview_document_height, preview_document_key,
    preview_document_max_scroll, preview_edit_bands, preview_image_placement,
    preview_link_activation, preview_link_answers_a_press, preview_opened_label,
    preview_page_browser_hand_off, preview_page_hand_off, preview_press_opens_its_link,
    preview_rail_tip_text, preview_selection_bands, preview_tab_index_among, preview_text_box_at,
    preview_wrap_columns, push_markdown_source_block, resolve_document_pictures,
    source_opens_as_a_page, step_preview_caret_by_row, strip_animation_tick_is_due, surface_pixels,
    surface_subject_of, surface_takes_image_zoom, surfaces_reading, switcher_rows,
    tab_trailing_targets, tick_owes_a_present, video_frame_texture_key, video_still_destination,
    viewport_of_rect, window_taskbar_progress, zoom_about,
};
// Its one test caller (`tests.rs`, a share handed over) is Windows-only, so the
// re-export carries the same gate.
#[cfg(all(test, windows))]
pub(crate) use preview::unverified_reference_handoff;
#[cfg(test)]
pub(crate) use profiles::profile_menu_anchor;
#[cfg(test)]
pub(crate) use search::rescan_leaf_for_search;
#[cfg(test)]
pub(crate) use tabs::{
    absorb_tab_into_layout, absorb_tab_into_strip, blank_page_return,
    exited_tabs_the_loop_may_close, grabbed_offset, new_tab_cwd, new_tab_leaf_seed,
    pane_can_become_a_tab, pane_strip_landing, row_strip_landing, stepped_tab, tab_close_action,
    tab_surface, two_tabs_mut,
};
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
    a_retirement_happens_on_this_turn, hole_for, page_destination, revived_page_of,
    switcher_row_destination,
};
mod terminal;
mod tooltips;
mod update_card;
mod web;
mod windows;
