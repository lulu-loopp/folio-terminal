//! `preview` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    ADDRESS_FIELD_WANTS_THE_WHOLE_HEAD, AnimationEntry, AnimationWork, AppEvent, AttentionDelivery,
    BackgroundDecode, BlockScrollPaint, ClipboardPictureAnswer, ClipboardPictureJob,
    DOCK_PREVIEW_FADE, DiffRow, DocumentMath, DocumentPictures, DragLatch, DrawnAnimation,
    DropPreview, FOOT_REVEAL_FEEDBACK, FileMenuTarget, FilePick, FootSaying, ForeignPane,
    FormulaSwitches, FrameTraces, ImageDrag, ImageGrasp, ImageZoom, ImeOwner, LeafId, LeafSeed,
    MARKDOWN_CHIP_RADIUS_LOGICAL_PX, MarkdownBlockLayout, MarkdownCaretBlock, MarkdownCaretPaint,
    MarkdownCaretSeat, MarkdownLive, MarkdownPage, MarkdownPicture, MarkdownPreedit,
    MarkdownProseBlock, MarkdownRaster, MarkdownRasterKey, MarkdownRasterRequest,
    MarkdownSourceBlock, MarkdownSourceBytes, MathWorkerRequest, Motion, NoticeHost,
    PREVIEW_REFUSAL_HOLD, PageArt, PageArtKey, PagePicture, PasteTarget, PeekCacheEntry,
    PeekThumbnailTarget, PictureErrand, PictureOnGlass, PictureReach, PictureRefusal,
    PopoverTrigger, Popup, PresentIntent, PreviewBlockDrag, PreviewBodyDrag, PreviewDocument,
    PreviewEditPaint, PreviewHeadFrame, PreviewHexHover, PreviewImageState, PreviewLink,
    PreviewLinkActivation, PreviewMathSite, PreviewOpenLane, PreviewPane, PreviewPreedit,
    PreviewRailFrame, PreviewSurface, PreviewTextDrag, PreviewTextSite, PreviewViewState,
    RenameSubject, Reparse, RevealTween, RevealedFoot, RowActivation, Runtime, ScaleWorkerRequest,
    ScrollThumbState, Step, SurfacePixels, SurfaceSubject, TabClick, TabId, TabRename, TabState,
    TabTriggerHand, VideoGlance, VideoShape, WINDOW_RESIZE_QUIET, WebHeadVerb, WindowRuntime,
    advance_drawn_animations, animation, animation_layer_key, animation_refusal_notice,
    animations_opened, answer_one_picture, answers_for, attention_trace, build_preview_diff_body,
    build_preview_markdown_body, build_preview_table_body, build_preview_text_body,
    clipboard_picture, create_leaf_session, crumb_segments, deliver_clipboard_picture, diagnostics,
    documents_held_in, documents_held_mut_in, documents_pictures, earliest_deadline, file_peek,
    files_a_tab_stands_on, files_row_display_name, float, float_dock_label, folded_levels,
    foot_revealed_label, forget_a_picture, forget_standing_answers, git_panel, graph_key_of,
    hang_watch, hex_peek, highlight, i18n, image_clamped_pan, image_destination, image_is_pannable,
    image_meta_sentence, image_raster_cap, image_zoom_caption, image_zoom_key, image_zoom_scale,
    image_zoom_settles, image_zoom_toggled, ime_owner, input, markdown_empty_page_offset,
    markdown_prose_composition, markdown_prose_face, markdown_prose_paragraphs, markdown_runs,
    markdown_source_cell, markdown_source_offset_at, marks, measure_preview_links,
    name_is_writable, native_window, notice, owe_sharpened_rasters, page_destination,
    page_foot_flash, page_foot_lead, page_source_file, peek_scale_task, picture_channel_owner,
    picture_errand, picture_files_of, pictures_awaited_by, pictures_need_handing_over,
    place_preview_math, preedit_caret_byte, present_diagnostics, present_drawn_animations, preview,
    preview_block_bar_at, preview_block_wheel, preview_body_bar, preview_caret_row,
    preview_copies_on_select, preview_document_height, preview_document_key,
    preview_document_max_scroll, preview_edit, preview_edit_bands, preview_image_placement,
    preview_link_activation, preview_link_answers_a_press, preview_live,
    preview_open_externally_label, preview_open_lane, preview_opened_label, preview_page_hand_off,
    preview_press, preview_press_opens_its_link, preview_provenance, preview_rail_tip_text,
    preview_select, preview_selection_bands, preview_tab_index_among, preview_text,
    preview_text_box_at, preview_text_boxes, preview_text_grain, preview_trace, preview_viewport,
    preview_watch, preview_wide_blocks, preview_wrap, preview_wrap_columns, profiles,
    prose_source_lines, recoverable_clipboard_write, resolve_document_pictures,
    revealable_preview_file, same_path_ignoring_case, sample_window_place, scroll_bar_layer,
    scrollback_quota, seats, settings, settle_attention, shown_address, source_opens_as_a_page,
    step_preview_caret_by_row, strip_animation_tick_is_due, surface_pixels, surface_subject_of,
    surface_takes_image_zoom, switcher_rows, tab_owes_frame, tab_trailing_targets, table_block,
    text_field, tick_owes_a_present, toast, tooltip, trace_sink, video_frame_texture_key,
    video_seat, video_still_destination, viewport_of_rect, visible_range, webhost, webnav,
    wheel_points_sideways, window_taskbar_progress, write_terminal_clipboard_text,
};
use anyhow::Context;
use anyhow::{Result, anyhow};
use bt_layout::SeatId;
use bt_render::{
    FrameSource, FrameTrigger, ImeCursorArea, PREVIEW_BODY_INSET_LOGICAL_PX, Preedit,
    PresentOutcome, PreviewImage, preview_image_extent,
};
use bt_term::normalized_local_image_path_key;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use winit::dpi::PhysicalPosition;
use winit::event::{Ime, KeyEvent, MouseScrollDelta};
use winit::keyboard::{Key, NamedKey};

impl Runtime<'_> {
    /// **Ask the worker for everything a revived tab's preview panes are
    /// showing.**
    ///
    /// A restored pool is a list of *names*: P151's "dirty edits do not survive"
    /// means every buffer comes back with no body at all, so the ones a pane is
    /// actually on have to be read before there is anything to draw. The rest
    /// stay empty until the switcher lands on one, and `open_preview_file_on`
    /// asks then — the same door, and the same lazy rule browsing already uses,
    /// so a tab restored with eight buffers does not read eight files to show
    /// one.
    ///
    /// By index rather than on the active tab, because a restore builds every
    /// tab before it activates any of them, and the response carries the
    /// [`TabId`] it was asked for.
    pub(crate) fn request_revived_previews(&mut self, index: usize) {
        let Some(tab) = self.window.tabs.get(index) else {
            return;
        };
        let id = tab.id;
        // A picture's question is named here because it is the pane's — the
        // decode lane owns a picture's arrival and there is no buffer ledger to
        // ask. A document's is *not*: which of the two reads it is owed, and the
        // state it is owed for, are the buffer's own answer, and since ticket
        // T-EDIT-DISK the one door that says both is the door that files the
        // question ([`preview::PreviewBuffer::claim_head_read`]). So this walk
        // names the files and the loop below asks.
        let wants: Vec<(preview::PreviewSource, Option<preview::PreviewWant>)> = tab
            .preview_panes
            .iter()
            .filter_map(|(_, pane)| {
                if let Some(image) = pane.image.as_ref() {
                    // The one field of the meta line no decoder can answer.
                    return Some((
                        preview::PreviewSource::file(image.path.clone()),
                        Some(preview::PreviewWant::Size),
                    ));
                }
                let source = pane.buffer.as_ref()?;
                tab.preview_pool
                    .get(source)
                    .filter(|buffer| buffer.wants_head_read())
                    .map(|_| (source.clone(), None))
            })
            .collect();
        // **A revived tab's pictures are opened, not remembered** (user report
        // 2026-08-31) — [`Self::open_preview_image_on`]'s line, at the other door
        // a `PreviewImageState` is born. A window that restores a tab it closed
        // an hour ago may still be holding the decode that tab was showing, and
        // the file behind it is an hour older than anything this window knows.
        let pictures: Vec<PathBuf> = self
            .window
            .tabs
            .get(index)
            .into_iter()
            .flat_map(|tab| tab.preview_panes.iter())
            .filter_map(|(_, pane)| Some(pane.image.as_ref()?.path.clone()))
            .collect();
        for path in pictures {
            self.forget_the_picture_in(&path);
        }
        for (source, asked) in wants {
            // The head lane's one door, for [`Self::arm_card_reads`]'s reason: a
            // buffer whose read is already out with the worker is not asked
            // again. A size is a question about a picture and has no such
            // ledger — the decode lane is what owns a picture's arrival.
            let want = match asked {
                Some(want) => want,
                None => {
                    let Some(want) = self
                        .window
                        .tabs
                        .get_mut(index)
                        .and_then(|tab| tab.preview_pool.get_mut(&source))
                        .and_then(preview::PreviewBuffer::claim_head_read)
                    else {
                        continue;
                    };
                    want
                }
            };
            if !self.app.preview_worker.request(preview::PreviewRequest {
                window: self.window_id(),
                tab: id,
                source,
                want,
            }) {
                self.disable_preview_worker();
                return;
            }
        }
    }

    /// The colour token under the pointer, as an anchor identity and the face its
    /// card is drawn in.
    pub(crate) fn preview_hex_anchor(
        &self,
    ) -> Option<(tooltip::TooltipAnchorId, tooltip::TipFace)> {
        let hover = self.window.preview_hex_hover.as_ref()?;
        Some((
            tooltip::TooltipAnchorId::PreviewHex(hover.surface, hover.offset),
            tooltip::TipFace::Swatch { rgba: hover.rgba },
        ))
    }

    /// **What the document on this surface has to say about its file** (user
    /// ruling 2026-08-29).
    ///
    /// The projection of [`preview::DiskNews`] onto the strip, and the whole of
    /// it. `Level` — the ordinary state of every buffer in this window — wears
    /// nothing, which is what keeps this row off every pane that has no news.
    ///
    /// Asked of a *surface* rather than of a seat since B1 (2026-09-01), because
    /// the second host has no seat: a torn-off window is a `PreviewSurface` and
    /// nothing else, and a question that could only be asked about a seat was
    /// the whole of why its reader was never told.
    pub(crate) fn preview_disk_notice_on(&self, surface: PreviewSurface) -> Option<notice::Notice> {
        let buffer = self.preview_buffer_on(surface)?;
        match buffer.disk {
            preview::DiskNews::Level => None,
            preview::DiskNews::Changed => Some(notice::Notice::DiskChanged),
            preview::DiskNews::Deleted => Some(notice::Notice::DiskDeleted),
        }
    }

    /// [`Self::settle_pane_notices`] under the name the watcher calls it by.
    ///
    /// One function and not two: the set of panes wearing a strip is one set,
    /// and a second settler for the preview half would be a second answer to
    /// "how tall is this body" — which is the drift the geometry's own note is
    /// about. The name exists so that the watcher's call site says what it
    /// means rather than reaching sideways into the shell integration's
    /// vocabulary.
    fn settle_preview_disk_notices(&mut self) -> Result<()> {
        self.settle_pane_notices()
    }

    /// **Take the disk's copy of this host's document** — the strip's `Reload`
    /// (user ruling 2026-08-29).
    ///
    /// The strip goes down in the same breath as the read goes out, because the
    /// sentence it carries has been answered; the *body* stays on the glass
    /// until the new one lands, which is `mark_stale`'s own rule and the reason
    /// nothing flashes.
    ///
    /// Asked of a host and resolved through `notice_surface`, so that the pane
    /// and the window re-read one buffer out of one pool by one door (B1).
    pub(crate) fn reload_preview_from_disk(&mut self, host: NoticeHost) -> Result<()> {
        let surface = self.notice_surface(host);
        if !self
            .preview_buffer_on_mut(surface)
            .is_some_and(preview::PreviewBuffer::take_the_disks_copy)
        {
            return Ok(());
        }
        // The pool this host reads, which is the tab the seat is on — or, for a
        // window, the tab it was torn off (`FloatPreview::tab`).
        let index = self.preview_tab_index(surface);
        self.request_stale_previews(index);
        self.settle_pane_notices()
    }

    /// The chosen picture's file name, for the button that carries it.
    ///
    /// The **name** and not the path: 118px of picker cannot hold
    /// `C:\\Users\\…\\Pictures\\ridge line.jpg`, and a path ellipsised from the
    /// right shows the part nobody needs. A file that has since been moved keeps
    /// its name on the button, because the setting still names it — see
    /// `bt_persist::DEFAULT_BACKGROUND_IMAGE` on why the path is not validated.
    pub(crate) fn background_image_name(&self) -> String {
        let stored = &self.app.settings_store.loaded().background_image;
        if stored.is_empty() {
            return String::new();
        }
        Path::new(stored)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| stored.clone())
    }

    /// **The picture of the pane in this window's hand, ready to travel** (B2).
    ///
    /// [`seats::FocusThumbnail`]'s three fields, owned. It is the projection the
    /// pane's *own* card is drawn from — one reading of one shell, which is
    /// §7.1.6b⁗'s own sentence — so a visitor's card in another window and the
    /// home card in this one cannot come to show two different things.
    ///
    /// `None` while this window holds no projection of that seat, and that is
    /// the honest answer rather than a gap: there is a difference between "the
    /// pane looks like this" and "nobody has looked". What removes most of that
    /// silence is [`Self::refresh_focus_thumbnails`], which projects a carried
    /// pane even in a window with no column of its own.
    pub(crate) fn carried_pane_picture(&self) -> Option<ForeignPane> {
        let leaf = self.carried_pane()?;
        let tab = self.tab_state(leaf.tab)?;
        let tree = bt_layout::LayoutNode::seat(tab.seats.tree().find_seat(leaf.seat)?.clone());
        let content = self
            .window
            .focus_thumbs
            .seats(leaf.tab)?
            .get(&leaf.seat)?
            .clone();
        Some(ForeignPane {
            tree,
            focused: leaf.seat,
            seats: BTreeMap::from([(leaf.seat, content)]),
        })
    }

    /// Bring the dock drawing up to date with the pointer, the tree and the
    /// window — M155's plan, its cache, and the fade.
    ///
    /// Called from [`Runtime::refresh_overlay`], which is the one choke point
    /// every repaint already passes through, so a resize, a DPI change or a theme
    /// switch re-plans without any of them having to know that a drag is in
    /// flight.
    pub(crate) fn sync_drop_preview(&mut self, now: Instant) {
        let motion = self.app.motion;
        let Some(inputs) = self.plan_inputs() else {
            self.retire_drop_preview(now, motion);
            return;
        };
        if self
            .window
            .drop_preview
            .as_ref()
            .is_none_or(|shown| shown.inputs != inputs)
        {
            let Some(plan) = self.plan_for(&inputs) else {
                // A landing whose plan cannot be built is not a landing that can
                // be drawn. It is not a refusal either — a refusal is a plan that
                // came out too small, and this is the aim naming a seat the tree
                // no longer has. The honest picture of a question with no answer
                // is no picture.
                self.retire_drop_preview(now, motion);
                return;
            };
            // The fade carries across, so moving between zones does not restart
            // it: the box was already up, and the answer changing is a *snap*
            // (M148), never a second arrival.
            let reveal = self.window.drop_preview.as_ref().map_or_else(
                || RevealTween::over(DOCK_PREVIEW_FADE),
                |shown| shown.reveal,
            );
            self.window.drop_preview = Some(DropPreview {
                inputs,
                plan,
                reveal,
            });
        }
        if let Some(shown) = self.window.drop_preview.as_mut() {
            shown.reveal.retarget(1.0, now, motion);
        }
    }

    /// Take the dock drawing down — `hidePreview()` (mock-up 6355-6367).
    ///
    /// It fades rather than vanishing, and it is kept alive for exactly as long
    /// as that takes: the mock-up removes `.show` and leaves the element in the
    /// document for its 100ms, which is what this state is standing in for. Once
    /// the box is gone the plan goes with it, because a plan nobody is drawing is
    /// an answer to a question nobody is asking.
    fn retire_drop_preview(&mut self, now: Instant, motion: Motion) {
        let Some(shown) = self.window.drop_preview.as_mut() else {
            return;
        };
        shown.reveal.retarget(0.0, now, motion);
        let (reveal, moving) = shown.reveal.sample(now, motion);
        if !moving && reveal <= 0.0 {
            self.window.drop_preview = None;
        }
    }

    /// Point the "Line wrapping" row at `wrapping`.
    ///
    /// **[`Self::adopt_new_language`]'s shape and not the row above's**, which is
    /// the whole of what is interesting here. `line_wrapping` is a member of
    /// [`bt_doc::LayoutKey`] — see `window_layout_key` — so it is not a setting
    /// anything has to be told about one object at a time. It is part of the
    /// identity of a laid-out document, and the road it travels is the one every
    /// other member of that key travels: move the stored answer, rebuild the key,
    /// and let `DualPlaneSession::set_layout_key` decide for itself whether what
    /// it is holding was laid out under the old one. A loop over every leaf
    /// calling a setter would be inventing a second channel beside the one the
    /// theme, the font, the language and the DPI all already use.
    ///
    /// **Every pane in every tab, and it has to be.** A split is two panes of one
    /// tab, both on screen, and no gesture the reader has re-keys the one that is
    /// not holding the keyboard: they would press `Off`, watch one half of their
    /// window stop folding, and have nothing at all that would persuade the other
    /// half. The road is therefore [`Self::apply_scale_factor`]'s — "no screen
    /// anywhere in the window is exempt from it" — for the same reason it is that
    /// function's, and [`Self::sync_math_layout_key`]'s for the same reason
    /// again: this is a fact about the product and not about the pane the
    /// keyboard happens to be in.
    ///
    /// **The key is amended rather than rebuilt.** No two panes share a width, so
    /// a key built here would have to go and find each pane's own columns to say
    /// anything true about it — and each session already holds a key that is
    /// right about everything except the one member that moved, so the one member
    /// is what changes. `DualPlaneSession::set_layout_key` compares and
    /// invalidates only where it actually differs, which is why a pane that was
    /// already reading this answer costs nothing.
    ///
    /// The publish is what puts the new answer on screen, in the frame the
    /// reader is watching.
    pub(crate) fn apply_line_wrapping(&mut self, wrapping: bool) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.line_wrapping = wrapping;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        for tab in &mut self.window.tabs {
            for (_, leaf) in tab.leaves_mut() {
                let amended = bt_doc::LayoutKey {
                    line_wrapping: wrapping,
                    ..leaf.session.layout_key()
                };
                leaf.session.set_layout_key(amended);
            }
        }
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    /// Lay one table out at this window's current metrics and inks.
    ///
    /// `source` is the block's **render source**: the rows exactly as the detector resolved them,
    /// one row to a line ([`bt_detect::table::TableSpan::resolved_source`]). The detector is the
    /// only place that holds the capture geometry a program's own wrap is read with, so the rows
    /// are resolved once, there, and written down — reading them back needs no geometry and can
    /// reach no other answer, which is why a rejoined row is still one row after a resize.
    ///
    /// `None` only when `source` is not a table, which cannot happen for a proven block and is
    /// answered honestly rather than asserted: the source travels through the transcript, and a
    /// function that took an unparseable one on trust would be a panic waiting for a reflow.
    pub(crate) fn build_table_block(&mut self, source: &str) -> Option<table_block::TableBlock> {
        let span = bt_detect::table::from_resolved_source(source)?;
        let metrics = table_block::metrics(self.window.renderer.metrics().font_size_px);
        let palette = bt_render::chrome_palette();
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        Some(table_block::build(
            &span,
            metrics,
            &palette,
            |cell, heading| {
                // See `table_block::build`: a printed table's dollars are not
                // this window's markup to read.
                let runs = markdown_runs(
                    cell,
                    &palette,
                    heading,
                    &DocumentMath::default(),
                    metrics.font_size,
                );
                renderer.measure_preview_paragraph_width(
                    gpu,
                    &runs,
                    metrics.font_size,
                    metrics.line_height,
                )
            },
        ))
    }

    /// Read the picture named in the settings, or clear it.
    ///
    /// **Off the event thread** (§7.1.6c-4d, user report 2026-08-18). 4b did
    /// this inline and wrote down why: the two moments it runs are
    /// startup-with-a-picture-already-chosen and the instant a modal chooser
    /// closed, and both already have somebody waiting. That argument was made
    /// against a screenshot and dies against a photograph — a 24 MP JPEG is
    /// hundreds of milliseconds of decode with a resample behind it, and a
    /// window that stops answering the keyboard for a quarter of a second is
    /// not being polite to anybody. The row applies the instant it is pressed,
    /// which is the latency that mattered; the picture lands a beat later.
    ///
    /// **The picture already on screen stays up while the next one decodes.**
    /// Clearing here would make every change of wallpaper a flash of the bare
    /// ground, and the ground with no picture is not a state the reader asked
    /// to see.
    ///
    /// A file that will not decode leaves the **name in the settings** and the
    /// window without a picture, and says so once in a card: a wallpaper on a
    /// drive that is not plugged in today is the ordinary case, and quietly
    /// erasing the setting would mean plugging the drive back in changed nothing.
    pub(crate) fn reload_background_picture(&mut self) -> Result<()> {
        let stored = self.app.settings_store.loaded().background_image.clone();
        // Every call withdraws whatever the last one asked for, `None` included:
        // a clear that raced a slow decode must win, and the generation is how
        // it does.
        let generation = self.window.background_decode.withdraw();
        if stored.is_empty() {
            self.window.background_picture = None;
            return Ok(());
        }
        let ceiling = self.background_picture_ceiling();
        let path = PathBuf::from(&stored);
        let file = path.file_name().map_or_else(
            || stored.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
        let slot = self.window.background_decode.slot();
        let proxy = self.app.event_proxy.clone();
        // In the workers' band: a wallpaper is never the reason a frame is late.
        bt_platform::spawn_at_priority(
            "background-picture",
            bt_platform::ThreadPriority::BelowNormal,
            move || {
                let result = bt_term::decode_background_image(&path, ceiling).map(|decoded| {
                    Arc::new(bt_render::BackgroundImage {
                        key: decoded.key,
                        rgba: decoded.rgba,
                        width_px: decoded.width_px,
                        height_px: decoded.height_px,
                    })
                });
                *slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(BackgroundDecode {
                    generation,
                    file,
                    result,
                });
                // After the answer is in the slot, never before.
                let _ = proxy.send_event(AppEvent::BackgroundPictureReady);
            },
        )
        .ok();
        Ok(())
    }

    /// The largest texture the ground will ever resolve, in physical pixels.
    ///
    /// **The biggest monitor attached to this machine**, and not the window: the
    /// ground quad covers the window, so the most texels anything can resolve
    /// out of it is the number of pixels the window can occupy — bounded by the
    /// largest screen it can be dragged onto and maximised on. Taking it from
    /// the window's current size instead would mean a picture re-decoded on
    /// every drag, or one frozen at whatever shape the window had when it was
    /// chosen. The screens do not change while a frame is being dragged.
    ///
    /// A machine that reports no monitors at all falls back to this window's own
    /// physical size, which is the only other fact available and is never zero.
    fn background_picture_ceiling(&self) -> (u32, u32) {
        let mut ceiling = (0_u32, 0_u32);
        for monitor in self.window.window.available_monitors() {
            let size = monitor.size();
            ceiling.0 = ceiling.0.max(size.width);
            ceiling.1 = ceiling.1.max(size.height);
        }
        if ceiling.0 == 0 || ceiling.1 == 0 {
            let inner = self.window.window.inner_size();
            ceiling = (inner.width.max(1), inner.height.max(1));
        }
        ceiling
    }

    /// Take whatever the picture worker left, if it is still an answer to the
    /// question that is being asked.
    pub(crate) fn adopt_background_picture(&mut self) -> Result<()> {
        // A picture the reader has already replaced — or one the reader cleared
        // while it was still decoding — is dropped in silence by `take_current`:
        // it answers a row nobody is looking at any more, and a card about it
        // would be a report on a decision that has been superseded.
        let Some(landed) = self.window.background_decode.take_current() else {
            return Ok(());
        };
        match landed.result {
            Ok(image) => {
                self.window.background_picture = Some(image);
                self.apply_window_ground()?;
            }
            Err(refusal) => {
                self.window.background_picture = None;
                self.apply_window_ground()?;
                self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    Some(i18n::Text::BackgroundPictureRefused.text().to_owned()),
                    i18n::background_picture_refused(&landed.file, &refusal),
                )?;
            }
        }
        Ok(())
    }

    /// The picture behind the window, chosen or cleared.
    pub(crate) fn apply_background_image(&mut self, path: String) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.background_image = path;
        if &settings == self.app.settings_store.loaded() {
            return Ok(false);
        }
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        self.reload_background_picture()?;
        self.apply_window_ground()?;
        Ok(true)
    }

    /// How the picture meets the window.
    pub(crate) fn apply_image_fit(&mut self, fit: bt_persist::BackgroundFitV1) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.background_fit = fit;
        if &settings == self.app.settings_store.loaded() {
            return Ok(false);
        }
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        self.apply_window_ground()?;
        Ok(true)
    }

    /// `Choose…` on the Background image row: queue the system's chooser.
    ///
    /// It opens in the folder the current picture came from, which is where a
    /// person changing their wallpaper is looking. A folder that has since gone
    /// is not this function's failure — the dialog opens at the system's default
    /// instead, exactly as the folder chooser's own start does.
    pub(crate) fn browse_for_background_image(&mut self) {
        let stored = self.app.settings_store.loaded().background_image.clone();
        let start = (!stored.is_empty())
            .then(|| Path::new(&stored).parent().map(Path::to_path_buf))
            .flatten();
        match self
            .window
            .image_picker
            .request(bt_platform::FilePickKind::Image, start.as_deref())
        {
            Ok(true) => self.window.image_pick_pending = Some(FilePick::BackgroundImage),
            Ok(false) => {}
            Err(error) => eprintln!("recoverable picture chooser failure: {error}"),
        }
    }

    /// Collect the picture chooser's answer, once, after its modal has shut.
    pub(crate) fn apply_image_pick_result(&mut self) -> Result<()> {
        let Some(result) = self.window.image_picker.take_result() else {
            return Ok(());
        };
        let Some(asked) = self.window.image_pick_pending.take() else {
            return Ok(());
        };
        let path = match result {
            // Cancelled. The row keeps whatever it had, which is the only
            // reading of Cancel that is not a clear.
            Ok(None) => return Ok(()),
            Ok(Some(path)) => path,
            Err(error) => {
                eprintln!("recoverable file chooser failure: {error}");
                return Ok(());
            }
        };
        match asked {
            FilePick::BackgroundImage => {
                self.apply_background_image(path.to_string_lossy().into_owned())?;
            }
            // **The row is asked for again**, because a chooser can stand open
            // for a minute and the table can move in that minute: a reorder or an
            // Undo would otherwise land somebody's program on the row that has
            // taken the index.
            FilePick::ProfileProgram => {
                let Some(index) = self.window.settings.editor().map(|editor| editor.index) else {
                    return Ok(());
                };
                if let Some(editor) = self.window.settings.editor_mut() {
                    editor.program = text_field::TextField::holding(&path.to_string_lossy());
                }
                profiles::set_program_path(index, &path);
                self.store_profiles()?;
            }
        }
        Ok(())
    }

    /// **Open the editor on the file a preview head names** (user ruling
    /// 2026-08-19).
    ///
    /// **A virtual document does not offer it — absent, not refused.** A git
    /// diff opened from a review has no file behind its name, and neither will
    /// anything else this pane learns to show that it did not read off the disk;
    /// [`preview::PreviewSource::file_path`] is the one door that answers, and
    /// the head simply stays a head. That is the same sentence a bundled colour
    /// scheme's missing marks make one dialog away.
    pub(crate) fn open_preview_rename(&mut self, seat: SeatId) -> Result<()> {
        let surface = self.preview_here(seat);
        let Some(buffer) = self.preview_buffer_on(surface) else {
            return Ok(());
        };
        if buffer.source.file_path().is_none() {
            return Ok(());
        }
        let (source, name) = (buffer.source.clone(), buffer.name.clone());
        self.window.rename = Some(TabRename::open_file(surface, source, &name));
        self.window
            .rename_blink
            .reset(Instant::now(), self.app.motion);
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// **Put a box on the breadcrumb's last segment** (B5, user ruling
    /// 2026-08-25).
    ///
    /// [`Self::open_preview_rename`] with the rail's subject instead of the
    /// head's, and everything after it is shared: the same seed, the same keys,
    /// the same commit onto the same disk. The tail of a path *is* the file the
    /// head names, so the two boxes could not be allowed to disagree about what
    /// happens when Enter is pressed in them.
    ///
    /// **The name is taken off the buffer and not off the crumb**, even though
    /// the crumb is what was double-clicked: `crumb_segments` draws the last
    /// segment from the path, and a path's last component and a buffer's name
    /// are the same string by construction — but only one of the two is the
    /// thing the commit will move.
    fn open_preview_crumb_rename(&mut self, surface: PreviewSurface) -> Result<()> {
        // A page's rail is an address and has no crumbs to double-click; a rail
        // that is not showing a document has nothing to rename.
        if self.rail_page(surface).is_some() {
            return Ok(());
        }
        let Some(buffer) = self.preview_buffer_on(surface) else {
            return Ok(());
        };
        if buffer.source.file_path().is_none() {
            return Ok(());
        }
        let (source, name) = (buffer.source.clone(), buffer.name.clone());
        self.window.rename = Some(TabRename::open_crumb(surface, source, &name));
        self.window
            .rename_blink
            .reset(Instant::now(), self.app.motion);
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// **The open editor, measured into the head it is drawn in** (user ruling
    /// 2026-08-19).
    ///
    /// It answers the pair `dress_preview_head` needs and nothing else: the
    /// `name_width` the head must lay its box out to, and the drawn state of the
    /// editor inside that box.
    ///
    /// **The box is sized to the draft, not to the head** — the mock-up's `size`
    /// attribute, which is the one width that is exactly the label's. Filling
    /// the head would shove the dirty dot and the tools to the far edge and drag
    /// them back on commit, which is a header rearranging itself around a name
    /// somebody is still typing. `preview_head_geometry` still clamps to what is
    /// left, so a very long draft is cut rather than allowed to push the tools
    /// off; the `first_visible` walk below is what keeps the caret inside that
    /// cut, and it is the tab editor's own loop for the tab editor's own reason.
    fn dress_preview_name_editor(
        &mut self,
        seat: SeatId,
        surface: PreviewSurface,
        scale: f32,
        tools: seats::PreviewHeadTools,
    ) -> (seats::PreviewHeadTools, Option<seats::TabEdit>) {
        // **The address field is no longer this editor** (user ruling
        // 2026-08-24). It was — same cell, same metrics, same box — for as long
        // as a page's name cell *was* its address; the ruling retired that, so
        // the field went down to the rail and took its measuring with it. See
        // [`Self::dress_preview_address_editor`], which is this function with
        // one box substituted for another.
        let editing = self.window.rename.as_ref().is_some_and(|editor| {
            matches!(&editor.subject, RenameSubject::PreviewName { surface: at, .. } if *at == surface)
        });
        if !editing {
            return (tools, None);
        }
        let font = seats::PREVIEW_NAME_FONT_LOGICAL_PX * scale;
        let draft = self
            .window
            .rename
            .as_ref()
            .map(|editor| editor.text().to_owned())
            .unwrap_or_default();
        // **The box is the width of the name being typed.** It used to fork
        // here — a file's box the width of its name, an address's the head's
        // whole remaining room, because 「URL 不是标签」 — and the fork went to
        // the rail on 2026-08-24 with the field it was about. What is left is
        // the half this cell always meant: a name is a label, and a label's box
        // is its label.
        // In the name's own face, not the face's regular weight: this box is
        // laid out by `preview_head_geometry` exactly as the committed name's
        // is, and a draft measured a weight light would put the caret in front
        // of the letters it is standing after. See [`seats::PREVIEW_NAME_FACE`].
        let tools = seats::PreviewHeadTools {
            name_width: self.window.renderer.measure_chrome_label(
                &mut self.app.gpu,
                &draft,
                font,
                seats::PREVIEW_NAME_FACE.weight,
                seats::PREVIEW_NAME_FACE.letter_spacing_em,
                seats::PREVIEW_NAME_FACE.tabular_numerals,
            ),
            ..tools
        };
        let Some(rect) = seats::full_pane_rect(&self.seat_layout, seat) else {
            return (tools, None);
        };
        let head = seats::pane_head_geometry(
            rect,
            bt_layout::SeatKind::Preview,
            self.seat_layout.seat_is_on_stage(seat),
            scale,
        );
        let name_box = seats::preview_head_geometry(&head, scale, tools).name;
        let box_width = name_box[2] - name_box[0];
        let caret_width = (seats::TAB_RENAME_CARET_LOGICAL_PX * scale)
            .round()
            .max(1.0);
        // Disjoint fields, split by hand: the editor owns where its window
        // starts and the renderer owns how wide a string is, and this is the one
        // place the two have to meet — `measure_open_rename`'s own division.
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let Some(editor) = self.window.rename.as_mut() else {
            return (tools, None);
        };
        // The name's own face, as the box above was: the caret and the
        // selection band are offsets *into* this run, so a run measured in
        // another weight puts both of them in the wrong place.
        let mut shape = |text: &str| {
            renderer.chrome_label_advances(
                gpu,
                text,
                font,
                seats::PREVIEW_NAME_FACE.weight,
                seats::PREVIEW_NAME_FACE.letter_spacing_em,
                seats::PREVIEW_NAME_FACE.tabular_numerals,
            )
        };
        // A file has no name under its name, so there is no layer to reveal and
        // the placeholder stays empty by construction rather than by omission.
        let edit = seats::TabEdit {
            caret_lit: self.window.rename_blink.visible(),
            ..editor.fit(box_width, caret_width, true, &mut shape)
        };
        // **Written here because here is the only place the box exists**, which
        // is `measure_open_rename`'s own sentence: the IME's candidate list has
        // to stand under the name being typed, and this rectangle is the one
        // thing that says where that is.
        let x = (name_box[0] + edit.caret_px).min(name_box[2] - caret_width);
        self.window.rename_caret_line = Some([x, name_box[1], x + caret_width, name_box[3]]);
        (tools, Some(edit))
    }

    /// **The open address editor, measured into the rail it is drawn in** (user
    /// ruling 2026-08-24).
    ///
    /// [`Self::dress_preview_name_editor`] with one box substituted for another,
    /// and written as a second function rather than as a branch inside that one
    /// because the two now lay out into different *rows*: a file's name is a
    /// label in the head that becomes a field, and an address is a field on the
    /// rail that is always a field. What they still share is the editor itself —
    /// one [`TabRename`], one caret, one selection, one set of exits — which is
    /// the whole of what "the machine changed address" means here.
    ///
    /// **The field takes the row's whole free run while it is being typed in**
    /// (§7.7 ②'s 「URL 不是标签」, unchanged): a width no row can grant, which
    /// `preview_rail_geometry`'s own clamp turns into "everything the buttons
    /// left". At rest the field is the width of the address, which is what lets
    /// it be centred.
    fn dress_preview_address_editor(
        &mut self,
        surface: PreviewSurface,
        scale: f32,
        measure_in: seats::PreviewRailMeasure,
    ) -> (seats::PreviewRailMeasure, Option<seats::TabEdit>) {
        // **The page this surface is showing, docked or torn off** (§7.7 ⑩ 欠账,
        // 2026-08-25). The editor was keyed by leaf on the day it was written
        // and is keyed by leaf still — what moved is only the question of which
        // leaf this *row* is about, which a closed seat could no longer answer.
        let here = self.rail_page(surface);
        let editing = self.window.rename.as_ref().is_some_and(
            |editor| matches!(editor.subject, RenameSubject::WebAddress { leaf: at } if Some(at) == here),
        );
        if !editing {
            return (measure_in, None);
        }
        let measure_in = seats::PreviewRailMeasure {
            address_width: ADDRESS_FIELD_WANTS_THE_WHOLE_HEAD,
            ..measure_in
        };
        let Some(band) = self.rail_band(surface, scale) else {
            return (measure_in, None);
        };
        let Some(field) = seats::preview_rail_geometry_in(band, scale, &measure_in).address else {
            return (measure_in, None);
        };
        let inset = (seats::PREVIEW_ADDRESS_PAD_X_LOGICAL_PX * scale).round();
        let box_ = [
            field[0] + inset,
            field[1],
            (field[2] - inset).max(field[0] + inset),
            field[3],
        ];
        let box_width = box_[2] - box_[0];
        let font = seats::PREVIEW_RAIL_FONT_LOGICAL_PX * scale;
        let caret_width = (seats::TAB_RENAME_CARET_LOGICAL_PX * scale)
            .round()
            .max(1.0);
        // Disjoint fields, split by hand, for `measure_open_rename`'s reason:
        // the editor owns where its window starts and the renderer owns how wide
        // a string is, and this is the one place the two have to meet.
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let Some(editor) = self.window.rename.as_mut() else {
            return (measure_in, None);
        };
        let mut shape = |text: &str| renderer.chrome_text_advances(gpu, text, font);
        // An address has no address under it either: the field is seeded with
        // the committed URL and there is no second layer to reveal.
        let edit = seats::TabEdit {
            caret_lit: self.window.rename_blink.visible(),
            ..editor.fit(box_width, caret_width, true, &mut shape)
        };
        // **Written here because here is the only place the box exists** — the
        // IME's candidate list has to stand under what is being typed, and this
        // is the rectangle that says where that is. It used to be the head's
        // name box; it is the rail's field now, which is the whole of what the
        // ruling moved as far as an IME is concerned.
        let x = (box_[0] + edit.caret_px).min(box_[2] - caret_width);
        self.window.rename_caret_line = Some([x, box_[1], x + caret_width, box_[3]]);
        (measure_in, Some(edit))
    }

    /// **The open crumb editor, measured into the segment it is drawn in** (B5,
    /// user ruling 2026-08-25).
    ///
    /// [`Self::dress_preview_address_editor`] with the tail crumb's box
    /// substituted for the address field's, and it is a third function rather
    /// than a branch inside either for that one's stated reason: these lay out
    /// into different boxes, and a box is the whole of what a caret's position
    /// is measured against.
    ///
    /// **There is no `first_visible` walk here, and that is a property of the
    /// box rather than an omission.** An address field has a fixed run and a URL
    /// longer than it, so it scrolls; a crumb's box was measured *from this very
    /// draft* one pass ago, so the draft always fits it exactly. What clamps a
    /// very long name is `preview_rail_geometry`'s own fold, which takes the
    /// middle of the path away before it takes room from the file.
    fn dress_preview_crumb_editor(
        &mut self,
        surface: PreviewSurface,
        scale: f32,
        measure_in: &seats::PreviewRailMeasure,
    ) -> Option<seats::TabEdit> {
        self.open_crumb_draft(surface)?;
        let geometry =
            seats::preview_rail_geometry_in(self.rail_band(surface, scale)?, scale, measure_in);
        let box_ = geometry
            .crumbs
            .iter()
            .find(|crumb| crumb.tail)
            .map(|crumb| crumb.rect)?;
        let box_width = box_[2] - box_[0];
        let font = seats::PREVIEW_RAIL_FONT_LOGICAL_PX * scale;
        let caret_width = (seats::TAB_RENAME_CARET_LOGICAL_PX * scale)
            .round()
            .max(1.0);
        // Disjoint fields, split by hand, for `measure_open_rename`'s reason: the
        // editor owns its draft and the renderer owns how wide a string is, and
        // this is the one place the two have to meet.
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let editor = self.window.rename.as_mut()?;
        let mut shape = |text: &str| renderer.chrome_text_advances(gpu, text, font);
        // The window never leaves the start of the draft, which is what the
        // `false` says: a file has no name under its name, so there is no layer
        // to reveal and the placeholder stays empty.
        let edit = seats::TabEdit {
            caret_lit: self.window.rename_blink.visible(),
            ..editor.fit(box_width, caret_width, false, &mut shape)
        };
        // **Written here because here is the only place the box exists** — the
        // IME's candidate list has to stand under what is being typed.
        let x = (box_[0] + edit.caret_px).min(box_[2] - caret_width);
        self.window.rename_caret_line = Some([x, box_[1], x + caret_width, box_[3]]);
        Some(edit)
    }

    /// **Commit a preview head's draft to the filesystem** (user ruling
    /// 2026-08-19).
    ///
    /// # Refusals are quiet, and the one that is not
    ///
    /// An empty name, a name that is unchanged, a name holding one of the nine
    /// characters Windows reserves, and a name a *different* file in that folder
    /// already has — every one of them leaves the name as it was and says
    /// nothing. This is a rename in a preview header, not a dialog: the reader
    /// can see whether it worked, because the name is right there and it either
    /// changed or it did not.
    ///
    /// **The filesystem's own refusal DOES speak**, and that is a deliberate
    /// departure from "refusals are quiet" rather than a hole in it. The
    /// mock-up's rule has a reason attached — the reader can see the answer — and
    /// it names its own exception: "the one refusal that DOES speak is the one
    /// the reader cannot see". A locked file is exactly that case. Every other
    /// refusal here is a fact about the *draft*, visible in the box the draft
    /// was typed in; a handle another program is holding is a fact about the
    /// machine, and a name that snaps back for no reason a reader can find is a
    /// reader pressing Enter again and again. So the four judgements above stay
    /// silent and `std::fs::rename` failing raises one error card on the pane
    /// the press came from — which is `save_preview_on`'s own answer to the same
    /// question one function along.
    ///
    /// # Why the collision is checked and not left to Windows
    ///
    /// `std::fs::rename` is `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`, so
    /// on this platform it would **silently overwrite** a file that already had
    /// the new name. A rename that eats somebody's file is not a rename, so the
    /// collision is decided here.
    ///
    /// It is decided *case-insensitively against the old path*, which is the
    /// other half: `notes.md` → `Notes.md` is a legitimate rename of one file
    /// into its own name, and on a case-insensitive filesystem the destination
    /// "already exists" because it IS the source. So a destination that exists
    /// refuses only when it is a different entry.
    ///
    /// # What follows the file
    ///
    /// The buffer's identity is its path, so the pool is re-keyed, every pane
    /// showing it is re-pointed, and the view store's memory of where the reader
    /// was in that document moves with it — three tables keyed by
    /// `PreviewSource`, all three of them, because a rename that updated two
    /// would leave the third answering for a file that is not there.
    ///
    /// **Nothing tells the watchers.** A scheme file renamed from this head
    /// moves the folder `SchemeWatch` is subscribed to, so the catalogue is
    /// re-read and `rescan_verdict`'s rename-follow rule runs — exactly as it
    /// does for a rename made in Explorer, and by the same code. A rename this
    /// window made must not be a special case, because the whole point of the
    /// follow rule is that it does not care who moved the file.
    pub(crate) fn rename_preview_file(
        &mut self,
        surface: PreviewSurface,
        source: &preview::PreviewSource,
        draft: &str,
    ) -> Result<()> {
        let Some(old) = source.file_path().map(std::path::Path::to_path_buf) else {
            return Ok(());
        };
        let Some(directory) = old.parent().map(std::path::Path::to_path_buf) else {
            return Ok(());
        };
        let name = draft.trim();
        let was = old
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        if name.is_empty() || name == was || !name_is_writable(name) {
            return Ok(());
        }
        let new = directory.join(name);
        if new.exists() && !same_path_ignoring_case(&old, &new) {
            return Ok(());
        }
        if let Err(error) = hang_watch::during(hang_watch::Station::RenameDisk, || {
            std::fs::rename(&old, &new)
        }) {
            let anchor = surface.toast_anchor();
            return self.toast(
                toast::ToastKind::Error,
                anchor,
                Some(was),
                i18n::not_renamed(&error.to_string()),
            );
        }
        self.follow_renamed_preview(source, &new, name);
        self.refresh_files_dirs_at(&directory);
        // **A suffix that changes the view can change the lane** (user ruling
        // 2026-08-23). `follow_renamed_preview` already re-asks `preview_ftype`
        // — "renaming `notes.md` to `notes.txt` really does turn a rendered
        // document into a text editor, which is the filesystem's answer and not
        // ours to soften" — and since the day a name can say *page*, the
        // filesystem's answer to `notes.md` → `notes.html` is a page. Left
        // here, the buffer would keep its text and be drawn as a document this
        // window has no reader for, which is exactly the contradiction this
        // ruling exists to end. The reverse cannot happen: a page has no file
        // path, so this door refuses it above.
        //
        // **On this surface, and this is §7.14f's rule rather than a second
        // one.** The pane whose head you just typed a name into is the pane the
        // renamed file is on; a door that already knows which one that is has no
        // business asking where a *newly opened* file would go. While the page
        // lane picked its own pane, renaming a locked pane's `notes.md` to
        // `notes.html` opened the page somewhere else and left that pane
        // pointing at a name the disk no longer has.
        if let Some(path) = source_opens_as_a_page(&preview::PreviewSource::file(&new)) {
            self.mark_session_dirty(Instant::now());
            return self.open_preview_web_file_on(surface, path);
        }
        // **The session holds the path, so the session has moved too.** A window
        // killed after a rename and restored from a file still naming the old
        // path opens on `No preview — file not found`, which is this window
        // telling the reader their own rename did not happen. The tab editor
        // marks the session for the same reason one function up.
        self.mark_session_dirty(Instant::now());
        Ok(())
    }

    /// Move one buffer's identity, and everything keyed by it, onto a new path.
    pub(crate) fn follow_renamed_preview(
        &mut self,
        source: &preview::PreviewSource,
        path: &std::path::Path,
        name: &str,
    ) {
        let moved = preview::PreviewSource::file(path);
        let index = self.window.active_tab;
        let tab = &mut self.window.tabs[index];
        // `take` then `insert` rather than a re-key method: it is the shape the
        // pool already offers and the shape the float migration already uses.
        if let Some(mut buffer) = tab.preview_pool.take(source) {
            buffer.source = moved.clone();
            // The suffix is what the body is drawn from, so a rename that
            // changes it changes the view — `notes.md` to `notes.txt` really
            // does turn a rendered document into a text editor, which is the
            // filesystem's answer and not ours to soften. **What a new name
            // cannot take back is the sniff** (§7.32): see
            // [`preview::PreviewBuffer::rename`], which is where both halves of
            // that live now.
            buffer.rename(name.to_owned());
            tab.preview_pool.insert(buffer);
        }
        for (_, pane) in tab.preview_panes.iter_mut() {
            if pane.buffer.as_ref() == Some(source) {
                pane.buffer = Some(moved.clone());
            }
        }
        self.preview_views.rekey(source, moved);
    }

    /// The storage folder moved and has gone quiet: read the two hand-editable
    /// files in it again (§7.1.6c-6d).
    ///
    /// **Bring the preview seats' subscriptions level and answer whatever the
    /// kernel has said** (W2 slice 5).
    ///
    /// [`Self::advance_git_watch`]'s shape one subject over, and the two
    /// halves are one call for that function's reason: the set a window is
    /// watching changes on the same events that produce news about it, and
    /// syncing after answering would act on a file the seat has already left.
    pub(crate) fn advance_preview_watch(&mut self, now: Instant) -> Result<()> {
        let wanted = self.watched_preview_files();
        self.window
            .preview_watch
            .sync(&wanted, &self.app.event_proxy);
        for news in self.window.preview_watch.due(now) {
            self.refresh_preview_file(&news)?;
        }
        Ok(())
    }

    /// **The second road: ask about the files no kernel is speaking for** (user
    /// ruling 2026-08-29).
    ///
    /// [`preview_watch::PreviewWatch::ask_the_unwatched`]'s two moments, and its
    /// whole answer routed through the same door the kernel's word goes through
    /// — so a share behaves exactly like a local disk once it has been asked,
    /// rather than nearly like one.
    ///
    /// The two moments are the ruling's: **the window is given focus** (whatever
    /// happened while it was away happened in another process, and coming back
    /// is when a reader is entitled to a true page — R31's own sentence for the
    /// git surfaces, read one lane over), and **a document is brought to the
    /// front** ([`Self::land_preview_source_on`], the one door every document
    /// arrives by). Neither is a clock.
    ///
    /// **The subscriptions are brought level first**, for
    /// [`Self::advance_preview_watch`]'s reason: a document that has just been
    /// added to a pool is not in the watch set until `sync` has run, and asking
    /// before that would be asking about the set as it was a gesture ago.
    pub(crate) fn ask_the_unwatched_preview_files(&mut self) -> Result<()> {
        let wanted = self.watched_preview_files();
        self.window
            .preview_watch
            .sync(&wanted, &self.app.event_proxy);
        for news in self.window.preview_watch.ask_the_unwatched() {
            self.refresh_preview_file(&news)?;
        }
        Ok(())
    }

    /// **Every file some preview buffer in this window stands on.**
    ///
    /// The gate `preview_watch` follows, and the whole of it.
    ///
    /// # The pool and not the panes (user ruling 2026-08-29)
    ///
    /// This used to ask the **panes** what they were showing, and that was the
    /// defect a reader reported on a real machine: a repository's `README.md`
    /// open in a preview pane with seven other documents behind it in the pool,
    /// rewritten on disk by a merge, still showing the old body — and **still
    /// showing it after switching away and back**, which is the sentence that
    /// names the true cause. A buffer is *content* and belongs to the tab
    /// (§7.1.3's shared pool): coming back to one does not re-read it, by
    /// design and rightly, because that is what makes an unsaved edit survive a
    /// switch. So a document that leaves the glass while the disk moves under it
    /// was never told, and nothing later would ever tell it — the stale body was
    /// the rest of the session.
    ///
    /// The subscription therefore follows **the pool**: every buffer the tab is
    /// holding, whether or not a pane is on it this second. The cost is
    /// bounded and small, and the two facts that bound it are the pool's own —
    /// a tab keeps at most eight clean unshown buffers (§7.1.3), and Windows
    /// subscribes to *folders*, so eight documents in one folder are eight
    /// clocks behind one handle.
    ///
    /// Four kinds of thing answer and they answer with the same thing — a path
    /// on a disk: a document through `PreviewSource::file_path`, a page through
    /// the URL its buffer is keyed by decoded back to the path it was minted
    /// from, a picture a markdown page is showing, and — since the report of
    /// 2026-08-31 — the picture a pane is *itself* showing, which is in no pool
    /// and was the one file this set left out. A page on a *server* and a
    /// git-backed document have no file behind them and contribute nothing,
    /// which is why this is one function and not four.
    ///
    /// **Across tabs, not only the tab on screen** - see
    /// [`preview_watch::PreviewWatch::sync`] for why.
    fn watched_preview_files(&self) -> BTreeSet<PathBuf> {
        self.window
            .tabs
            .iter()
            .flat_map(files_a_tab_stands_on)
            .collect()
    }

    /// **Every picture file a markdown page in this window is showing.**
    ///
    /// A second walk beside [`files_a_tab_stands_on`]'s picture arm rather than a
    /// reading of it, because the two answer different questions with the same
    /// paths: this one is asked of *one path that has just moved* — "is this a
    /// picture some page is standing on, and therefore a decode to throw away" —
    /// and that one is asked of the whole window. Deriving one from the other
    /// would put the window's whole set on the path of a single notification.
    ///
    /// The picture the watch follows and the picture a decode landing asks about
    /// are the same set, so it is derived in one place from the documents
    /// themselves — see [`DocumentPictures::files`]. The comment on
    /// [`Self::watched_preview_files`] used to say a picture was deliberately
    /// *not* watched; that was true of the image *pane*, whose pixels arrive
    /// down the decode lane and whose file is the thing the pane is showing.
    /// A picture inside a document is a different object: the document is what
    /// the reader opened, the picture is part of how it reads, and 「图片文件改了
    /// 重画」 is the ruling of 2026-08-28.
    fn markdown_picture_files(&self) -> BTreeSet<PathBuf> {
        picture_files_of(self.documents_held())
    }

    /// **Every surface in this window that is holding a document** — the docked
    /// panes and the floats of every tab, and the glance card (§7.1.3u ③).
    ///
    /// One helper because there are exactly three holders and one of them was
    /// being left out of every walk that mattered. A walk over `tab.preview_panes`
    /// finds the seats and the floats, because a float's view is in its tab's own
    /// map; it cannot find [`PreviewSurface::Peek`], whose view is the *window's*
    /// — one pointer, one card — and lives on [`WindowRuntime::peek_pane`]. The
    /// card really does build a markdown document (`rebuild_preview_document`
    /// through [`Self::refresh_overlay`]), so it really does have pictures it is
    /// waiting for, files it stands on and standing answers that a moved file
    /// must end; it was in none of the three sets that say so, and a hover over a
    /// markdown file with an uncached picture in it stayed on placeholders until
    /// something unrelated happened to bump the picture generation.
    ///
    /// [`Self::animated_surfaces`]' argument said about documents instead of
    /// about frames: the card is added by hand for the reason `preview_surfaces`
    /// leaves it out — it is window-scoped rather than tab-scoped, and not
    /// focusable, hit-testable or scrollable — and it is added because it draws,
    /// which is what these walks are about.
    ///
    /// **Across tabs, not only the tab on screen**, for
    /// [`Self::watched_preview_files`]' reason: a page in a background tab is
    /// still a page holding an answer about a file that can move.
    fn documents_held(&self) -> impl Iterator<Item = &PreviewPane> {
        documents_held_in(&self.window.tabs, &self.window.peek_pane)
    }

    /// The same, mutably — what ends a standing answer walks it.
    fn documents_held_mut(&mut self) -> impl Iterator<Item = &mut PreviewPane> {
        documents_held_mut_in(&mut self.window.tabs, &mut self.window.peek_pane)
    }

    /// **Which pictures a page in this window is still waiting to see**
    /// (§7.1.3u, second report).
    ///
    /// [`Self::markdown_picture_files`]'s narrower sibling, and the narrowing is
    /// the point: that one is every file a page's pictures came from, which is
    /// the right set to *watch* and the wrong set to **re-flow** on. A decode
    /// landing owes a page a new layout only if the page would come out
    /// differently for it, and a page that is already drawing that picture — at
    /// whatever size it has — would not. Re-flowing it anyway is a full re-shape
    /// of every paragraph on the page for pixels nothing will use, which is the
    /// other half of what pinned a core while a reader typed into a README full
    /// of screenshots.
    ///
    /// See [`DocumentPictures::loading`] for what a page writes down here.
    fn markdown_pictures_awaited(&self) -> BTreeSet<PathBuf> {
        pictures_awaited_by(self.documents_held())
    }

    /// **Which tabs hold a pane standing on the picture in this file** (user
    /// report 2026-08-31).
    ///
    /// [`Self::markdown_picture_files`]'s opposite number, for the *other* place
    /// a picture is shown: the picture lane's own pane, whose file is
    /// [`PreviewImageState::path`] and is in no pool. Tabs and not surfaces
    /// because the one thing the caller does with the answer is address a
    /// question to the worker, and a `PreviewRequest` is addressed to a tab —
    /// two panes of one tab on one file are one question, which is what a
    /// deduplicated set of ids says.
    ///
    /// Every surface, seat and float alike: a picture popped out into a window
    /// of its own is the same file on the same disk.
    fn tabs_showing_the_picture_in(&self, path: &Path) -> BTreeSet<TabId> {
        self.window
            .tabs
            .iter()
            .filter(|tab| {
                tab.preview_panes
                    .iter()
                    .any(|(_, pane)| pane.image.as_ref().is_some_and(|it| it.path == path))
            })
            .map(|tab| tab.id)
            .collect()
    }

    /// **Forget everything this window remembers about the picture in one file**
    /// (user report 2026-08-31).
    ///
    /// One door and three callers, because the memo is one memo: the watcher's
    /// news about a file, a picture being opened onto a pane, and a restored tab
    /// coming back holding one. What each of those has in common is that this
    /// window is about to draw a file it has not looked at since it last wrote
    /// something down about it.
    ///
    /// Three things are written down and all three go:
    ///
    /// * the **native decode** ([`WindowRuntime::peek_cache`]), which is keyed by
    ///   the path and would otherwise hand out the old picture for the new file.
    ///   Removing it is what makes the next frame ask —
    ///   [`Self::refit_preview_picture`] requests a decode for a picture the
    ///   cache has no entry for, and refills `native` from the answer, so the
    ///   dimensions on the meta line follow the pixels without being told;
    /// * the **sentence a video's own container states**
    ///   ([`WindowRuntime::video_facts`]), keyed identically and stale on exactly
    ///   the same terms;
    /// * the **generation of the exact-size rasters**
    ///   ([`MarkdownPictures::forget`]), which is how every markdown page
    ///   standing on this file is told to ask again.
    ///
    /// It asks no disk and reads no file. What it does is end this window's
    /// right to answer from memory; the asking is the frame's, down the lane it
    /// already has.
    ///
    /// The mechanism is [`forget_a_picture`], on
    /// [`files_a_tab_stands_on`]'s own argument: a `Runtime` needs a device
    /// layer and a window to exist, and what a test has to be able to put in
    /// front of this is three real caches.
    fn forget_the_picture_in(&mut self, path: &Path) {
        forget_a_picture(
            &mut self.window.peek_cache,
            &mut self.window.video_facts,
            &mut self.window.markdown_pictures,
            path,
        );
        // **And the answers the surfaces themselves are standing on.** Since
        // [`answer_one_picture`] a page keeps what it was told when the decode
        // cache lets the pixels go, and since §7.1.3u ③ a picture pane does the
        // same — which is what stops a bounded cache from sending this window
        // round the same nine decodes forever, and is exactly what must *not*
        // survive a file moving under it. This is the one place the three
        // ledgers are ended together, so it is the one place the fourth is ended
        // too, and it is ended for **every** holder: the card's document is one
        // ([`Self::documents_held`]).
        forget_standing_answers(self.documents_held_mut(), path);
    }

    /// **One watched file moved: tell whatever is showing it** (W2 slice 5).
    ///
    /// Two lanes and the plan names both:
    ///
    /// * **a page takes an ordinary `Reload`** ("网页座位刷新 = 一次正常
    ///   `Reload`(不是重新导航)"). A navigation would truncate the page's own
    ///   history and re-run the whole policy for an address that has not
    ///   changed; what changed is the bytes behind where the engine already is,
    ///   and that is precisely what `Reload` means.
    /// * **a document re-reads its head** ("文档座位刷新 = 重读那一份头"), through
    ///   the same worker, the same one-question ledger and the same landing that
    ///   opening it used. `PreviewBuffer::mark_stale` refuses a buffer with
    ///   unsaved edits, which is the ruling that keeps a save in another window
    ///   from destroying work in this one.
    ///
    /// Every tab, because a buffer is content and belongs to a tab: two tabs on
    /// one file are two buffers and both are behind the disk.
    fn refresh_preview_file(&mut self, news: &preview_watch::FileNews) -> Result<()> {
        let path = news.path.as_path();
        // **A picture is a picture wherever it is standing** (§7.1.3k ④; user
        // report 2026-08-31). Two surfaces show one, and until the report they
        // were treated as two different questions: a picture *inside* a markdown
        // page was forgotten here, and a picture a pane was itself showing was
        // not mentioned at all — so a PNG replaced under its own name went on
        // being drawn from a decode nothing would ever throw away.
        //
        // They are one question, and the answer is one door
        // ([`Self::forget_the_picture_in`]). What is left over is the byte count
        // on a pane's meta line, which is the one field of it no decoder can
        // answer and which therefore has to be asked again by name.
        let pictured = self.tabs_showing_the_picture_in(path);
        if !pictured.is_empty() || self.markdown_picture_files().contains(path) {
            self.forget_the_picture_in(path);
            for tab in pictured {
                if !self.app.preview_worker.request(preview::PreviewRequest {
                    window: self.window_id(),
                    tab,
                    source: preview::PreviewSource::file(path.to_path_buf()),
                    want: preview::PreviewWant::Size,
                }) {
                    self.disable_preview_worker();
                }
            }
            self.refresh_preview_for_layout();
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        // **The pane and not the seat number** (F1b′): this walk is over every
        // tab, and a seat number means nothing outside the tab it was read from.
        let reload: Vec<LeafId> = self
            .window
            .tabs
            .iter()
            .flat_map(|tab| {
                tab.preview_panes
                    .iter()
                    .map(move |(surface, pane)| (tab.id, surface, pane))
            })
            .filter_map(|(tab, surface, pane)| {
                let PreviewSurface::Seat(leaf) = surface else {
                    return None;
                };
                debug_assert_eq!(
                    leaf.tab, tab,
                    "a tab's preview map is filed under that tab's own leaves"
                );
                let url = pane.buffer.as_ref()?.web_url()?;
                let named = webnav::LocalFileUrl::parse(url)?;
                (named.path() == path && self.window.web.contains_key(&leaf)).then_some(leaf)
            })
            .collect();
        for leaf in reload {
            let window = &mut *self.window;
            let outcomes = window
                .web
                .get_mut(&leaf)
                .map(|web| web.reload(&window.compositor))
                .unwrap_or_default();
            self.apply_web_outcomes(leaf, outcomes)?;
        }
        // **Every tab's pool and not every tab's panes** (user ruling
        // 2026-08-29). This walk was always over the pools; what changed with the
        // ruling is that the *watch* now reaches the buffers no pane is on, so
        // this walk finally has something to say about them — and the answer is
        // routed through [`preview::PreviewBuffer::note_disk_moved`], which is
        // the one door that knows the ruling's three cases apart.
        let source = preview::PreviewSource::file(path);
        let mut read_again = Vec::new();
        let mut said = false;
        for (index, tab) in self.window.tabs.iter_mut().enumerate() {
            let Some(buffer) = tab.preview_pool.get_mut(&source) else {
                continue;
            };
            match buffer.note_disk_moved(news.present, news.modified) {
                preview::DiskVerdict::Nothing => {}
                preview::DiskVerdict::ReadAgain => read_again.push(index),
                preview::DiskVerdict::Say => said = true,
            }
        }
        for index in read_again {
            self.request_stale_previews(index);
        }
        if said {
            // A strip that has appeared or changed what it says is a row of
            // somebody's document, so this goes through the same door the
            // terminal's own strip does — which re-solves the seats when the set
            // of panes wearing one moves, and repaints when only the words did.
            self.settle_preview_disk_notices()?;
        }
        Ok(())
    }

    /// **Ask again for every pooled buffer the disk has moved under** (user
    /// ruling 2026-08-29).
    ///
    /// [`Self::request_revived_previews`]'s sibling, and a second function
    /// rather than a widening of that one because the two are asking opposite
    /// questions of the same pool. A *revived* tab's buffers are all empty, and
    /// that door is deliberately lazy — reading eight files to show one is what
    /// its own note forbids. A *stale* buffer already has a body somebody read;
    /// it is behind its file by exactly one write, and the pane it is not on
    /// today is the pane somebody switches back to tomorrow expecting the truth.
    /// So this one walks the pool and that one walks the panes, and each is
    /// right about its own case.
    ///
    /// Through the one ledger either way ([`preview::PreviewBuffer::claim_head_read`]),
    /// so a file open in two panes and the pool is read once.
    fn request_stale_previews(&mut self, index: usize) {
        let Some(tab) = self.window.tabs.get(index) else {
            return;
        };
        let id = tab.id;
        let stale: Vec<preview::PreviewSource> = tab
            .preview_pool
            .buffers()
            .filter(|buffer| buffer.is_behind_the_disk())
            .map(|buffer| buffer.source.clone())
            .collect();
        for source in stale {
            // The one door files the question *and* says which of the two reads
            // it took (T2 ③): a document that bought the whole file is re-read
            // whole, so a save in another window does not quietly put the
            // reader back on the first 64KB of what they are editing.
            let Some(want) = self
                .window
                .tabs
                .get_mut(index)
                .and_then(|tab| tab.preview_pool.get_mut(&source))
                .and_then(preview::PreviewBuffer::claim_head_read)
            else {
                continue;
            };
            if !self.app.preview_worker.request(preview::PreviewRequest {
                window: self.window_id(),
                tab: id,
                source,
                want,
            }) {
                self.disable_preview_worker();
                return;
            }
        }
    }

    /// What this surface is showing, or nothing if it has never shown anything.
    pub(crate) fn preview_pane(&self, surface: PreviewSurface) -> Option<&PreviewPane> {
        if surface == PreviewSurface::Peek {
            // The glance is the window's — one pointer, one card — so its view is
            // not in any tab's map. See [`PreviewSurface::Peek`].
            return Some(&self.window.peek_pane);
        }
        self.preview_tab(surface)?.preview_panes.get(surface)
    }

    /// The same, mutably and vivifying — a surface that exists has a view.
    pub(crate) fn preview_pane_mut(&mut self, surface: PreviewSurface) -> &mut PreviewPane {
        if surface == PreviewSurface::Peek {
            return &mut self.window.peek_pane;
        }
        let index = self.preview_tab_index(surface);
        self.window.tabs[index].preview_panes.entry(surface)
    }

    /// The buffer this surface is on, **mutably**, in its own tab's one pool.
    ///
    /// The mutable twin of [`Self::preview_buffer_on`] and it exists for the same
    /// sentence: the pool a surface edits is the pool it reads. A float carried
    /// across a tab switch that saved into whichever pool was on screen would be
    /// writing one file's keystrokes into another tab's copy of it.
    pub(crate) fn preview_buffer_on_mut(
        &mut self,
        surface: PreviewSurface,
    ) -> Option<&mut preview::PreviewBuffer> {
        // The same fork [`Self::preview_buffer_on`] makes, for its own stated
        // reason: the pool a surface edits is the pool it reads, and so is the
        // *buffer*.
        let flipped = self.page_source_shown_on(surface).map(Path::to_path_buf);
        let index = self.preview_tab_index(surface);
        let source = match flipped {
            Some(path) => preview::PreviewSource::file(path),
            None => self.window.tabs[index]
                .preview_panes
                .get(surface)?
                .buffer
                .clone()?,
        };
        self.window.tabs[index].preview_pool.get_mut(&source)
    }

    /// The buffer this surface is on, looked up in **its own** tab's one pool.
    ///
    /// The glance falls back past the pool: P145 gives it the tab's buffer
    /// whenever there is one — "so the glance never lies about unsaved edits" —
    /// and its own off-pool slot when there is not.
    pub(crate) fn preview_buffer_on(
        &self,
        surface: PreviewSurface,
    ) -> Option<&preview::PreviewBuffer> {
        if surface == PreviewSurface::Peek {
            let source = self.window.peek_pane.buffer.as_ref()?;
            return self.preview_pool.get(source).or_else(|| {
                self.window
                    .peek_buffer
                    .as_ref()
                    .filter(|buffer| &buffer.source == source)
            });
        }
        // **A page turned to its source reads that file's buffer** (user ruling
        // 2026-08-26). One fork, at the door every host asks its body, its view,
        // its ftype and its facts through — so the flip is one sentence rather
        // than a condition repeated at each of them. `pane.buffer` is *not*
        // redirected and must not be: that field is which content this seat is
        // on, the page is still what it is on, and it is what keeps the browser
        // ([`a_page_was_replaced`]).
        let path = self.page_source_shown_on(surface).map(Path::to_path_buf);
        let tab = self.preview_tab(surface)?;
        if let Some(path) = path {
            return tab.preview_pool.get(&preview::PreviewSource::file(path));
        }
        tab.preview_pool
            .get(tab.preview_panes.get(surface)?.buffer.as_ref()?)
    }

    /// Whether this surface is showing its buffer's **source** face (P28).
    ///
    /// The view's own answer and not the buffer's — see [`PreviewPane::md_source`]
    /// for the ruling. A surface that has never been flipped is showing the
    /// render, which is what `false` means and why the default is right.
    fn preview_md_source(&self, surface: PreviewSurface) -> bool {
        self.preview_pane(surface)
            .is_some_and(|pane| pane.md_source)
    }

    /// **The file whose source this surface's `</>` would turn to**, whichever
    /// face it is showing right now (user ruling 2026-08-26; DESIGN §7.7 ⑭).
    ///
    /// [`page_source_file`] asked of the page this surface is standing on, and
    /// the whole of what the rail's button set is drawn from: a rail carries
    /// `</>` when this answers, and does not when it does not. Asked of the
    /// *offer* rather than of the state, so the button does not vanish under the
    /// hand that just pressed it.
    pub(crate) fn preview_page_source_file(&self, surface: PreviewSurface) -> Option<PathBuf> {
        page_source_file(&self.web_of(surface)?.page().url)
    }

    /// **What paints this surface's body**, asked once, for every host.
    ///
    /// The picture is asked first and off the *pane* rather than off a buffer,
    /// because a picture is not one: it arrives down the decode lane and lives
    /// on [`PreviewPane::image`], and a surface holding one has no buffer to ask
    /// [`preview::PreviewBuffer::view`] of. Everything else is the buffer's own
    /// answer put through [`preview::PreviewView::chrome`], and an empty surface
    /// falls to the document pipeline, which is where the "no preview" card is
    /// drawn.
    fn preview_chrome_on(&self, surface: PreviewSurface) -> preview::PreviewChrome {
        if self
            .preview_pane(surface)
            .is_some_and(|pane| pane.image.is_some())
        {
            return preview::PreviewChrome::Picture;
        }
        let md_source = self.preview_md_source(surface);
        self.preview_buffer_on(surface)
            .map_or(preview::PreviewChrome::Document, |buffer| {
                buffer.view(md_source).chrome()
            })
    }

    /// Whether **this surface** has anything to type into.
    ///
    /// The buffer's own judgement asked with this surface's face, because that
    /// is precisely where the two faces of a markdown file differ: one is a
    /// rendered page with nowhere to put a caret, the other is text. Every gate
    /// that used to ask the buffer alone asks through here, so two surfaces on
    /// one file can disagree about it and both be right.
    ///
    /// **A page's source is the editor** (user ruling 2026-08-27; DESIGN §7.32,
    /// overturning §7.7 ⑭'s read-only half).
    ///
    /// The 2026-08-26 ruling gave a local page a source face and left it a
    /// reading, and it said in as many words which two questions it was not
    /// answering: what the live document on the glass does with a save, and
    /// which of `Save` and `DevTools` gets the head's one verb slot. Both are
    /// answered now and neither needed a new surface.
    ///
    /// * **The live document takes a reload.** The file a preview seat is
    ///   showing is already watched ([`preview_watch`]), and a page whose file
    ///   moves already takes an ordinary `Reload` — see
    ///   [`Self::refresh_preview_file`], which is the door a save in *another*
    ///   editor has always come through. A save in this one is that same event
    ///   with this window as the writer, so the page behind the source face
    ///   comes back current without a second mechanism and without the flip
    ///   knowing anything about it.
    /// * **The two verbs do not share a slot.** They never did: the head lays
    ///   out `save`, `flip`, `stop`, `devtools`, `popout` and `lock` as six
    ///   boxes ([`seats::preview_head_geometry`]), so a page showing its source
    ///   wears a `Save` beside its `</>` and its developer tools, and the
    ///   collision the earlier ruling reserved judgement on turned out not to
    ///   exist.
    ///
    /// So nothing is special-cased here at all. The buffer under the source face
    /// is an ordinary text buffer — that is the whole point of the promotion —
    /// and it answers this question the way every other text buffer in this
    /// window does, including its refusals: a truncated head is read-only
    /// wherever it is shown, and so is a `.pdf`, which has no source face to
    /// begin with.
    pub(crate) fn preview_is_editable(&self, surface: PreviewSurface) -> bool {
        let md_source = self.preview_md_source(surface);
        self.preview_buffer_on(surface)
            .is_some_and(|buffer| buffer.is_editable(md_source))
    }

    /// **Whether this surface is the rendered face of a Markdown file that can
    /// be typed into** (T5, §7.1.3t).
    ///
    /// The one question that separates the two editors this window now has, and
    /// it is asked wherever they would otherwise both answer: the press ladder
    /// (a rendered page is not the quick edit's `<textarea>`), the vertical
    /// motion (rows of a block against rows of a file), and the composition's
    /// caret box. Both faces of one buffer are editable now, so
    /// [`Self::preview_is_editable`] alone can no longer tell them apart.
    ///
    /// It says nothing about whether a caret is *in* the page — that is
    /// [`PreviewPane::md_caret`] — because the two are asked at different
    /// moments: this is asked by the press that is about to put one there.
    fn preview_shows_live_markdown(&self, surface: PreviewSurface) -> bool {
        if self.preview_md_source(surface) {
            return false;
        }
        self.preview_buffer_on(surface).is_some_and(|buffer| {
            buffer.view(false) == preview::PreviewView::Markdown && buffer.is_editable(false)
        })
    }

    /// **Somebody has asked to edit what is on this surface** — buy the whole
    /// file if the glance only bought its head (T2 ③, owner's ruling on research
    /// §10 Q2, 2026-09-10).
    ///
    /// [`Self::preview_is_editable`]'s active twin, and the reason it is a
    /// separate door rather than a line inside that one: `preview_is_editable`
    /// is asked by every frame that draws a head button, and a disk read on the
    /// strength of a frame is a disk read sixty times a second. This is asked by
    /// the two *gestures* that mean it — see
    /// [`preview::PreviewBuffer::ask_for_the_whole_file`], which names them and
    /// says why they are the two:
    ///
    /// * the flip to the source face of a Markdown buffer
    ///   ([`Self::flip_preview_source_on`]), which is the moment a rendered page
    ///   becomes something with a caret in it; and
    /// * a press inside the body of a surface whose *face* edits
    ///   ([`Self::press_preview_body`]), which is the moment a reader of a text
    ///   file becomes its writer. It is hung above that method's editability
    ///   gate rather than beside `preview_edit_focus`, and both halves of that
    ///   are deliberate: the gate is what a truncated buffer fails, so a trigger
    ///   below it could never fire for the files this exists for; and
    ///   `preview_edit_focus` is re-read on every frame, so a disk read hung
    ///   there would be sixty a second.
    ///
    /// The read goes out on the ordinary lane, through the ordinary ledger, and
    /// lands through the ordinary door — so the whole document arrives *on top
    /// of* the head that is already on the glass rather than in place of it, and
    /// the page does not flash. That is [`preview::PreviewBuffer::mark_stale`]'s
    /// standing behaviour and nothing here is a second copy of it.
    fn ask_to_edit_preview_on(&mut self, surface: PreviewSurface) {
        let Some(tab) = self.preview_tab_id(surface) else {
            return;
        };
        let window = self.window_id();
        let md_source = self.preview_md_source(surface);
        // The same door every host asks this surface's body through, so a page
        // turned to its source asks about the *file's* buffer and not about the
        // page's.
        let Some(buffer) = self.preview_buffer_on_mut(surface) else {
            return;
        };
        if !buffer.ask_for_the_whole_file(md_source) {
            return;
        }
        let Some(want) = buffer.claim_head_read() else {
            return;
        };
        let source = buffer.source.clone();
        if !self.app.preview_worker.request(preview::PreviewRequest {
            window,
            tab,
            source,
            want,
        }) {
            self.disable_preview_worker();
        }
    }

    /// **Whose content plane a surface reads from.**
    ///
    /// A seat belongs to the tab whose tree it is in, which is the active one at
    /// every site that can name a seat. A **float does not**: §7.1.2 gives a
    /// pinned window the run of the whole application — it floats over every tab,
    /// because that is the point of tearing something off — while `preview_panes`
    /// and the pool it reads are the *tab's*, by the 2026-07-17 ownership ruling.
    ///
    /// So a float's reads go through the tab it was torn out of, and not through
    /// `Deref`'s active one. Read the other way, a preview float drew an empty
    /// head, an empty foot and no body the moment you switched tabs — the window
    /// still there, still yours, and showing nothing.
    ///
    /// **A seat's reads go the same way since §7.12 ⓑ**, and for what turns out
    /// to be the same reason said about the other surface. This arm used to be
    /// `Some(self)` — "a seat is the front tab's, at every site that can name
    /// one" — while [`Self::preview_tab_index`] beside it searched the strip for
    /// a tab holding that seat *number*. Two tabs previewing on one number made
    /// those two different tabs, so a surface was read in one and written in the
    /// other. Both are now the one question `leaf.tab` already answers.
    fn preview_tab(&self, surface: PreviewSurface) -> Option<&TabState> {
        let tab = self.preview_tab_id(surface)?;
        self.window.tabs.iter().find(|state| state.id == tab)
    }

    /// **Which tab a preview surface's content belongs to, by name.**
    ///
    /// One question, one answer, and no search: a seat says so in its own
    /// [`LeafId`], a float says so on the window it was torn into, and the
    /// glance is the window's — it reads the tab on screen, which is the tab
    /// whose rows the pointer is over, because there is nowhere else a file row
    /// can be.
    fn preview_tab_id(&self, surface: PreviewSurface) -> Option<TabId> {
        match surface {
            PreviewSurface::Seat(leaf) => Some(leaf.tab),
            PreviewSurface::Peek => Some(self.id),
            PreviewSurface::Float(id) => Some(self.window.float.live(id)?.preview()?.tab),
        }
    }

    /// The same as an index, for the paths that need it mutably.
    ///
    /// Falls back to the active tab, and that fallback is reachable exactly once:
    /// [`Self::pop_out_preview`] writes the new window's view before the window
    /// is in the host to be asked about, and the tab it means is the active one
    /// by construction.
    pub(crate) fn preview_tab_index(&self, surface: PreviewSurface) -> usize {
        self.preview_tab_id(surface)
            .and_then(|tab| preview_tab_index_among(&self.window.tabs, tab))
            .unwrap_or(self.window.active_tab)
    }

    /// Every preview surface that exists **right now**, in paint order: the tree's
    /// preview leaves in seat order, then the floats bottom-to-top.
    ///
    /// Derived rather than stored, and that is the discipline this whole slice
    /// rests on — the layout tree and the float host are the two registers of
    /// what exists, and a third list beside them is a third thing that can be
    /// wrong. [`Self::sweep_preview_panes`] is the one place the derived answer
    /// is used to retire views, so a surface cannot outlive its surface.
    pub(crate) fn preview_surfaces(&self) -> Vec<PreviewSurface> {
        let mut surfaces: Vec<PreviewSurface> = self
            .seats
            .preview_seats()
            .into_iter()
            .map(|seat| self.preview_here(seat))
            .collect();
        // Every float, not only this tab's: the sweep below reads this list to
        // decide what has *stopped* existing, and a window torn out of another tab
        // is still standing. Listing only the active tab's would have every tab
        // switch retire the other tabs' views.
        surfaces.extend(
            self.window
                .float
                .drawn()
                .filter(|win| win.preview().is_some())
                .map(|win| PreviewSurface::Float(win.epoch)),
        );
        surfaces
    }

    /// **Shut down every recording whose surface has stopped being about it**
    /// (route B slice ②, 2026-08-28; §7.44 ③).
    ///
    /// One rule for all three surfaces, and it is the honest one: a seat is
    /// alive while the surface it is keyed by is still showing the file it was
    /// opened on. That covers every way a video ends without the stop button —
    /// a pane handed another document, a float closed, a card whose pointer
    /// moved to the next row, a tab torn away — without any of those doors
    /// having to remember a decoder exists.
    ///
    /// **A surface that has not yet said what it is showing keeps its seat.**
    /// The one moment that matters is a tear-off: the float is opened, the
    /// engine is handed over, and the picture arrives a call later. A sweep that
    /// read the missing picture as "not about this file" would shut down the
    /// engine it was just given, which is the re-open this ruling refused,
    /// arriving by the back door.
    ///
    /// **Answers whether the set of seats moved** (closure review 2, 2026-09-18):
    /// a membership change is one of the three things that can make the layers
    /// the renderer is holding wrong, and the service above this one rebuilds
    /// them on that answer rather than on every turn — see
    /// [`Self::service_pictures`].
    fn sweep_video_seats(&mut self) -> bool {
        // **A window holding no recording has nothing to retire** (closure
        // review O4, 2026-09-18). This used to run once per strip tick; it now
        // runs on every turn and at the head of every compose, because it is a
        // service, so the one case that is overwhelmingly the common one has to
        // cost a `is_empty()` rather than a walk of every surface this window
        // draws.
        if self.window.video.is_empty() {
            return false;
        }
        // **Which surfaces still exist**, asked once: a float that has finished
        // its exit fade and a pane that has been closed are both gone from this
        // list, and both take their decoder with them. Asked *here* and not
        // through the picture, because a surface that has gone has no picture
        // either — and "no picture" is also what a surface one call old looks
        // like, which is the tear-off. Two questions, because they are two
        // facts.
        let alive = self.preview_surfaces();
        let doomed: Vec<PreviewSurface> = self
            .window
            .video
            .iter()
            .filter(|(surface, seat)| {
                let subject = match surface {
                    PreviewSurface::Peek => {
                        // A card with no subject is a card that has gone; a card
                        // over another row is another file.
                        match self.file_peek_subject().and_then(|it| it.path) {
                            Some(path) => SurfaceSubject::File(path),
                            None => return true,
                        }
                    }
                    // **A window that has been closed is not playing
                    // anything**, and it stops on the press rather than at the
                    // end of its own leaving (§7.44 ⑨, found on the machine
                    // 2026-08-28). `preview_surfaces` is drawn from
                    // `float.drawn()`, which deliberately keeps a window that is
                    // on its way out so its *view* is not retired under it; a
                    // decoder is not a view, and the picture stayed on the glass
                    // — with the chassis already gone from over it — for as long
                    // as the departure took.
                    PreviewSurface::Float(id) if self.window.float.live(*id).is_none() => {
                        return true;
                    }
                    _ => {
                        if !alive.contains(surface) {
                            return true;
                        }
                        // **Every lane, not the picture lane** (§7.44 ⑬). This
                        // used to read `preview_picture`, so a pane handed a
                        // markdown file — which fills no picture — looked exactly
                        // like the tear-off that has not filed one yet, and kept
                        // its decoder. `SurfaceSubject` is the distinction:
                        // nothing filed anywhere is the grace, and something
                        // filed that is not this file is the end of the seat.
                        self.preview_subject(*surface)
                    }
                };
                !subject.is_still(seat.path())
            })
            .map(|(surface, _)| surface)
            .collect();
        let mut membership_moved = !doomed.is_empty();
        for surface in doomed {
            self.window.video.close(surface);
        }
        // **And an engine that gave up after it had opened** (§7.44 ⑥).
        //
        // `EngineError` is sticky — an engine that has errored does not
        // un-error — and it can arrive at any time: a codec that fails on a
        // frame a minute in, a file that was replaced under the decoder, a
        // container that turned out to be truncated. A seat left standing on one
        // is a rectangle that will never receive another picture, so it is shut
        // down and the surface is given the same sentence a refused *open*
        // gives, out of the same place.
        let faulted: Vec<(PreviewSurface, bt_platform::video::engine::EngineError)> = self
            .window
            .video
            .iter()
            .filter_map(|(surface, seat)| Some((surface, seat.fault()?)))
            .collect();
        membership_moved |= !faulted.is_empty();
        for (surface, error) in faulted {
            self.mouse_trace(|| format!("video_seat surface={surface:?} fault={error:?}"));
            self.window.video.close(surface);
            if let Some(picture) = self.preview_picture_mut(surface) {
                picture.failure = Some(PictureRefusal::this_window_cannot(
                    i18n::Text::VideoFormatCannotPlay.text(),
                ));
            }
        }
        membership_moved
    }

    /// Drop the view of every surface that has stopped existing.
    ///
    /// Called from the one door every seat-set change goes through and from every
    /// float closer, because a view left behind is a buffer this tab still
    /// believes is on screen — and the pool's dirty gates read exactly that
    /// belief, so a stale entry is a gate that stops asking about a file nobody
    /// can see any more.
    pub(crate) fn sweep_preview_panes(&mut self) {
        let alive = self.preview_surfaces();
        // **Every tab's map, not only the active one.** A float is torn out of the
        // tab that owned the pane and keeps reading that tab's plane wherever you
        // go ([`Self::preview_tab`]), so the window that closes while you are
        // somewhere else has to be retired *there*. Seat surfaces are unaffected:
        // a seat only ever appears in its own tab's map, so the `alive` list —
        // which carries this tab's seats and every window — leaves the other tabs'
        // seat entries exactly where they are.
        let dropped: Vec<(usize, PreviewSurface)> = self
            .window
            .tabs
            .iter()
            .enumerate()
            .flat_map(|(index, tab)| {
                tab.preview_panes
                    .iter()
                    .map(move |(surface, _)| (index, surface))
            })
            .filter(|(_, surface)| match surface {
                // A seat lives in its own tab's map and nowhere else, so only its
                // own tab's tree can say whether it is gone — and `alive` is the
                // tree that is solved, which is the tab on the glass. Since
                // §7.12 ⓑ the surface says which tab that is instead of the walk
                // having to compare its own index against the active one.
                PreviewSurface::Seat(leaf) => leaf.tab == self.id && !alive.contains(surface),
                PreviewSurface::Float(_) => !alive.contains(surface),
                // Unreachable, and that is the point: the glance's view lives on
                // the window ([`WindowRuntime::peek_pane`]), so no tab's map can be
                // holding one for the sweep to find. It is retired by the card
                // coming down, in `hide_file_peek`.
                PreviewSurface::Peek => false,
            })
            .collect();
        for (index, surface) in dropped {
            // Filed and cleared where it actually lives — the window's own tab,
            // which is not the one on screen whenever a float outlived a switch.
            self.leave_preview_buffer_in(index, surface);
            self.window.tabs[index].preview_panes.remove(surface);
            // **And what that surface was showing an animation of.** The record
            // is deliberately not cleared when a surface stops being *drawn* —
            // that is the whole of how a tab switch resumes rather than restarts
            // (see [`WindowRuntime::animation_presence`]) — so the one moment it
            // may be dropped is the moment the surface stops existing, which is
            // here.
            self.window.animation_presence.remove(&surface);
        }
        // And this tab's head measurements beside them. A seat id is re-minted
        // from a counter, so a width left behind for a pane that has gone comes
        // back as the next preview's head laid out to a name nobody chose for it
        // — the same argument `close_pane` makes about a files column's cached
        // root. Only seats have measured heads: a float's head is measured into
        // its own layer every frame and stored nowhere.
        self.window
            .preview_head_measures
            .retain(|leaf, _| alive.contains(&PreviewSurface::Seat(*leaf)));
        // **And the row's, on both hosts** (§7.7 ⑩ 欠账, 2026-08-25). The
        // sentence above is the whole argument and the only thing that differs
        // is the reach: a rail is measured for a window as well as for a seat,
        // and a float's epoch is minted from a counter exactly as a seat id is,
        // so a width left behind for a window that has gone would come back as
        // the next pop-out's row laid out to a path nobody is looking at. A seat
        // on another tab is left alone for the seat entries' own reason — `alive`
        // is the tree that is solved.
        let here = self.id;
        self.window.preview_rail_measures.retain(|surface, _| {
            alive.contains(surface)
                || matches!(surface, PreviewSurface::Seat(leaf) if leaf.tab != here)
        });
        if self
            .preview_edit_focus
            .is_some_and(|surface| !alive.contains(&surface))
        {
            self.preview_edit_focus = None;
        }
        if self
            .preview_selecting
            .is_some_and(|surface| !alive.contains(&surface))
        {
            self.preview_selecting = None;
        }
        if self
            .preview_text_drag
            .as_ref()
            .is_some_and(|drag| !alive.contains(&drag.surface))
        {
            self.preview_text_drag = None;
        }
        // **The card is not in `alive` and must not be swept out by its
        // absence.** [`PreviewSurface::Peek`] is not in the tree and not in the
        // float host — that is the same sentence the `Peek` arm above answers
        // `false` with — so its liveness is `file_peek`'s alone, and
        // [`Self::hide_file_peek`] is where a gesture on the card is let go.
        // Without the guard, any seat-set change would drop a hand that is in
        // the middle of dragging a table sideways inside a glance.
        if self.preview_block_drag.is_some_and(|drag| {
            drag.surface != PreviewSurface::Peek && !alive.contains(&drag.surface)
        }) {
            self.preview_block_drag = None;
        }
        if self.preview_block_hover.is_some_and(|(surface, _)| {
            surface != PreviewSurface::Peek && !alive.contains(&surface)
        }) {
            self.preview_block_hover = None;
        }
        if self
            .preview_body_drag
            .is_some_and(|drag| !alive.contains(&drag.surface))
        {
            self.preview_body_drag = None;
        }
        if self
            .preview_body_hover
            .is_some_and(|(surface, _)| !alive.contains(&surface))
        {
            self.preview_body_hover = None;
        }
        if self
            .preview_link_hover
            .as_ref()
            .is_some_and(|(surface, _)| !alive.contains(surface))
        {
            self.preview_link_hover = None;
        }
    }

    /// **Where a newly opened file lands** — P69 and P95 in one sentence.
    ///
    /// The un-pinned preview pane if the tab has one, and a fresh preview leaf at
    /// the ruled far-right address if it does not. This is the whole of what the
    /// pin buys: a pinned pane is not the answer to this question, so the next
    /// file opens *beside* it rather than over it.
    ///
    /// It never lands in a float. A float is somewhere a buffer was carried to by
    /// hand, and a browsing gesture that replaced what you had torn off would be
    /// the singleton reaching into a window you took out of its way on purpose.
    pub(crate) fn preview_landing_surface(&mut self) -> Option<PreviewSurface> {
        let metrics = self.seat_metrics();
        let existed = self.seats.landing_preview().is_some();
        // **Both `None`s are traced, separately, and neither is the same
        // finding** (`BT_MOUSE_TRACE`). The first is the layout refusing to mint
        // a leaf; the second is a leaf that *was* minted and then filed under a
        // settle that failed — which leaves the tree holding a pane the caller
        // is about to be told does not exist.
        let Some(seat) = self.seats.add_preview(&metrics) else {
            self.mouse_trace(|| {
                format!(
                    "preview_landing_surface none=add_preview reused={}",
                    u8::from(existed)
                )
            });
            return None;
        };
        if !existed && let Err(error) = self.settle_seat_set_change() {
            self.mouse_trace(|| {
                format!("preview_landing_surface none=settle_seat_set_change seat={seat:?} error={error:#}")
            });
            return None;
        }
        self.mouse_trace(|| {
            format!(
                "preview_landing_surface seat={seat:?} reused={}",
                u8::from(existed)
            )
        });
        Some(self.preview_here(seat))
    }

    /// The box **this surface's document** lives in — its body, whole.
    ///
    /// **It holds no furniture and gives up no height to any** (user ruling,
    /// 2026-08-15). There used to be a second derivation here: the body less a
    /// 28px read-only bar, for a truncated buffer only. The bar retired when the
    /// ruling moved the fact it stated into the right hand of the path strip
    /// below it, and with it went the one reason a document's box ever depended
    /// on what was in the document. One rectangle, asked one way, for every
    /// buffer.
    pub(crate) fn preview_surface_body_rect(
        &self,
        surface: PreviewSurface,
        scale: f32,
    ) -> Option<[f32; 4]> {
        match surface {
            // **This tab's tree is the only one solved**, so a pane on a tab you
            // are not looking at has no rectangle — which is what `None` says,
            // and what every caller already does the right thing with. Before
            // §7.12 ⓑ the seat number alone could not tell the two apart and
            // this handed back the box of a pane in front of you.
            PreviewSurface::Seat(leaf) => (leaf.tab == self.id)
                .then(|| {
                    seats::preview_seat_body_rect(&self.seats, &self.seat_layout, leaf.seat, scale)
                })
                .flatten(),
            PreviewSurface::Float(id) => self.float_body_rect(id, scale),
            // The card has no *pane* to be asked about: it is not in the tree and
            // not in the float host, it is a drawing placed beside a row. The one
            // caller that needs its box already has it and hands it in — see
            // [`Self::build_preview_body_in`].
            PreviewSurface::Peek => None,
        }
    }

    /// Which surface a point is inside, and the box of the document it is in.
    ///
    /// **The floats first, topmost first**, then the tree's preview leaves in
    /// tree order. A float is drawn over the panes, so a point inside a window
    /// standing across a preview pane belongs to the window — the same order
    /// [`Self::float_hit_at`] asks in, and the only one that agrees with what is
    /// on the glass.
    ///
    /// The one door every pointer question about a preview goes through. Before
    /// slice 5 there was nothing to resolve: "the preview" was a singleton, and
    /// each gesture asked for its body directly. With the plane plural the
    /// gesture has to say *whose* body it landed in before it can say anything
    /// else, and asking that in one place is what stops two of them disagreeing.
    pub(crate) fn preview_surface_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(PreviewSurface, [f32; 4])> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (x, y) = (position.x as f32, position.y as f32);
        self.window
            .float
            .hit_order()
            .filter(|win| win.preview().is_some())
            .map(|win| PreviewSurface::Float(win.epoch))
            .chain(
                self.seats
                    .preview_seats()
                    .into_iter()
                    .map(|seat| self.preview_here(seat)),
            )
            .find_map(|surface| {
                let body = self.preview_surface_body_rect(surface, scale)?;
                (body[0] <= x && x <= body[2] && body[1] <= y && y <= body[3])
                    .then_some((surface, body))
            })
    }

    /// The preview seat holding **layout focus**, if that is what has it.
    ///
    /// `seats.focus()` names a leaf of any kind; this is that leaf when it is a
    /// preview one. The singleton this replaces asked whether `seats.preview()`
    /// *was* the focus, which stopped being an answer the moment a tab could
    /// hold two preview leaves — it would have named the first of them however
    /// far the focus was from it.
    pub(crate) fn focused_preview_seat(&self) -> Option<SeatId> {
        let seat = self.seats.focus();
        self.seats.preview_seats().contains(&seat).then_some(seat)
    }

    /// The preview **float** holding window focus, if one does.
    ///
    /// `FloatWin::focused` is set the moment a click tore the window off and is
    /// exclusive across the host, so this is at most one window — the same
    /// singleton the files tree's own float focus is.
    fn focused_preview_float(&self) -> Option<float::FloatId> {
        self.window
            .float
            .live_windows()
            .find(|win| win.focused && win.preview().is_some())
            .map(|win| win.epoch)
    }

    /// **Which surface a keystroke is about.**
    ///
    /// Three readings in one order, and the order is the answer to "where is the
    /// user looking":
    ///
    /// 1. the quick edit's surface, when one holds the keyboard — that is
    ///    `InputOwner::PreviewEdit` naming itself;
    /// 2. a **focused preview float**, because a window somebody tore off by hand
    ///    and then clicked into is the thing the keys are for; a docked pane
    ///    keeping them would be the keystroke going to the pane the window is
    ///    standing over;
    /// 3. the focused preview leaf — the browsing half of the same owner (§7.1.5:
    ///    a focused preview is not a terminal, so its arrows scroll rather than
    ///    reach a shell).
    ///
    /// One question rather than one per key, because the states are set by
    /// different gestures and a key that consulted only one of them would work
    /// from a click in the body and not from a click on the head.
    pub(crate) fn preview_keyboard_surface(&self) -> Option<PreviewSurface> {
        self.preview_edit_focus()
            .or_else(|| self.focused_preview_float().map(PreviewSurface::Float))
            .or_else(|| {
                self.focused_preview_seat()
                    .map(|seat| self.preview_here(seat))
            })
    }

    /// Land a picture on the surface a newly opened file goes to, then ask the shared
    /// worker/cache pipeline for it. Keyboard focus deliberately remains on the terminal.
    pub(crate) fn open_preview_image(&mut self, path: PathBuf) -> Result<()> {
        self.mouse_trace(|| format!("open_preview_image enter path={}", path.display()));
        // Where it lands, before anything is written: `preview_landing_surface`
        // may have to mint a leaf, and a picture filed against a pane that then
        // failed to open is a picture nothing will ever draw.
        let Some(surface) = self.preview_landing_surface() else {
            // The silent door (`BT_MOUSE_TRACE`): from the outside this `Ok(())`
            // and a click that never happened are the same event.
            self.mouse_trace(|| "open_preview_image leave=no-landing-surface".to_owned());
            return Ok(());
        };
        self.mouse_trace(|| format!("open_preview_image leave=opened surface={surface:?}"));
        self.open_preview_image_on(surface, path)
    }

    /// The same, on a surface the caller has already chosen.
    ///
    /// **The split is what a drop needed** (P84): "a preview's centre shows it
    /// here" names the pane, and the landing rule must not be re-asked — the
    /// pane you aimed at is the pane that takes the file, whether or not it is
    /// the one an ordinary open would have chosen. The reuse rule and the
    /// *filling* of a surface were one function until this slice, which meant
    /// every door had to accept `landing_preview()`'s answer.
    fn open_preview_image_on(&mut self, surface: PreviewSurface, path: PathBuf) -> Result<()> {
        // **Opening a picture reads it** (user report 2026-08-31), which until
        // this ticket was the one thing about a picture that was not true. A
        // document opened onto a pane goes to the disk for its head; a picture
        // was served out of [`WindowRuntime::peek_cache`], keyed by its path and
        // never invalidated, so the plainest gesture a reader has — close the
        // pane, open the same file again — showed the picture that file used to
        // hold. The memo goes, the frame asks, and the answer is whatever is on
        // the disk now.
        //
        // It is cheap when nothing moved: the decoder's own memo is keyed by the
        // file's modified time and length since this ticket, so an unchanged
        // file costs one `metadata` on a worker thread and no decode at all.
        self.forget_the_picture_in(&path);
        // The one field of the meta line no decoder can answer, asked on the
        // same lane a document's head goes down.
        let tab = self.id;
        if !self.app.preview_worker.request(preview::PreviewRequest {
            window: self.window_id(),
            tab,
            source: preview::PreviewSource::file(path.clone()),
            want: preview::PreviewWant::Size,
        }) {
            self.disable_preview_worker();
        }
        // The document this surface was showing is not what it is showing now.
        // The buffer itself stays in the pool — it belongs to the tab, not to the
        // pane — so switching back finds it whole, and so does the view of it.
        self.leave_preview_buffer(surface);
        self.preview_pane_mut(surface).image = Some(PreviewImageState::new(path));
        // A new picture arrives fit to the pane. The zoom is the *view of one
        // picture* and not a setting of the surface: inheriting 250% and a pan
        // from the screenshot you were reading a moment ago would open the next
        // file somewhere in its own top-left corner for no reason anybody could
        // reconstruct.
        self.preview_pane_mut(surface).zoom = ImageZoom::FIT;
        // **Nothing to claim.** A picture landing used to take a one-seat texture
        // lane off whichever pane was holding it, which is how a recording pane
        // dropped beside a picture pane blanked the picture (user report
        // 2026-09-06; §7.1.6k⁷). The channel is plural now, so the picture that
        // has just landed is simply one more entry in the list
        // [`Self::refresh_preview_for_layout`] rebuilds below.
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Open a file on the surface a newly opened file lands on — the document
    /// door.
    ///
    /// [`Self::open_preview_image`]'s sibling and its opposite number: one takes
    /// a picture down the decode lane, this takes everything else through the
    /// tab's shared pool. Both leave the keyboard exactly where it was, which is
    /// §7.1.3's browsing continuity ("打开时键盘焦点不离开树") and the reason a
    /// tree can be walked more than once.
    ///
    /// **The pool answers first.** A file already open is the same buffer, its
    /// edits intact, whichever pane showed it — so this is a *lookup* that
    /// sometimes reads a disk, never a read that sometimes finds a cache.
    pub(crate) fn open_preview_file(&mut self, path: PathBuf) -> Result<()> {
        self.mouse_trace(|| format!("open_preview_file enter path={}", path.display()));
        // **The reuse target is `landing_preview()`** — the un-pinned preview
        // pane, and a fresh leaf beside the pinned one when there is no such
        // pane. That is the whole of what the pin has meant since slice 4 (P95:
        // "this pane keeps its buffers and stops being the reuse target — the
        // NEXT file opens a fresh preview beside it"), and it is the plural
        // content plane that finally lets it be acted on: a second surface can
        // hold a second buffer, a second caret and a second scroll, so landing
        // beside a pinned pane no longer means two heads over one document.
        //
        // Resolved before a single field is written, because it may have to mint
        // a leaf and a buffer filed against a pane that never opened is a buffer
        // nothing will ever draw.
        let Some(surface) = self.preview_landing_surface() else {
            // The silent door (`BT_MOUSE_TRACE`): from the outside this `Ok(())`
            // and a click that never happened are the same event.
            self.mouse_trace(|| "open_preview_file leave=no-landing-surface".to_owned());
            return Ok(());
        };
        self.mouse_trace(|| format!("open_preview_file leave=opened surface={surface:?}"));
        self.open_preview_file_on(surface, path)
    }

    /// **Which door a path goes through, on a named surface** — the drop's own
    /// opener.
    ///
    /// [`Self::open_preview`]'s split, minus the landing rule: a picture goes
    /// down the decode lane and everything else goes through the tab's pool,
    /// exactly as they do for a double-click, and the surface is the caller's.
    pub(crate) fn open_preview_onto(
        &mut self,
        surface: PreviewSurface,
        path: PathBuf,
    ) -> Result<()> {
        // **Through [`preview_open_lane`] and not through a second reading of the
        // name** (§7.10 ⑥). This door used to ask `path_is_previewable_image`
        // directly, which was the same answer while there were two lanes and
        // stopped being one the day a video became a third: a dropped `.mp4`
        // would have gone to the pool and drawn nothing, because the pool's
        // buffer for it says "picture" and the pane it landed on has no picture.
        match preview_open_lane(&path) {
            PreviewOpenLane::Picture | PreviewOpenLane::Video => {
                self.open_preview_image_on(surface, path)
            }
            // The page arm is the pool door's own, one call further in — see
            // [`Self::open_preview_source_on`], which is where every document
            // that is really a page turns around.
            PreviewOpenLane::Page | PreviewOpenLane::Document => {
                self.open_preview_file_on(surface, path)
            }
        }
    }

    /// **The controlled file entry, for a caller that has no pane in mind** (Web
    /// 预览块 W2 片⑤; `plan.md` section 3「受控 file 入口」) — a local page,
    /// wherever a newly opened file goes.
    ///
    /// The entry's own four steps live one call further in, on
    /// [`Self::open_preview_web_file_on`], which is where they always belonged:
    /// they are what a `file:` target costs, and they do not depend on which pane
    /// is going to show it. **This door owns the pane and nothing else.**
    ///
    /// **The seat is the landing rule's, and this door is the one that asks it**
    /// (§7.14f, 2026-09-06). The first unlocked preview pane, or a freshly split
    /// one when this tab has none — [`Self::preview_landing_surface`], the same
    /// answer every other kind of preview gets. A caller that already knows the
    /// pane it means goes through [`Self::open_preview_web_file_on`] instead and
    /// never reaches this line.
    ///
    /// **And the pane it chose takes the keyboard**, which is the pair
    /// [`Self::open_web_page_with`] has always been: a door that picks the pane
    /// is a door that put a browser somewhere you were not looking, so it moves
    /// you there. A refusal card is not a page and moves nothing.
    fn open_preview_web_file(&mut self, path: PathBuf) -> Result<()> {
        self.mouse_trace(|| format!("open_preview_web_file enter path={}", path.display()));
        let Some(surface) = self.preview_landing_surface() else {
            // The silent door (`BT_MOUSE_TRACE`): from the outside this `Ok(())`
            // and a click that never happened are the same event.
            self.mouse_trace(|| "open_preview_web_file leave=no-landing-surface".to_owned());
            return Ok(());
        };
        self.open_preview_web_file_on(surface, path)?;
        match surface {
            PreviewSurface::Seat(leaf) if self.seat_holds_a_page(leaf.seat) => {
                self.focus_seat(leaf.seat)
            }
            _ => Ok(()),
        }
    }

    /// **The controlled file entry, on a surface the caller has already named**
    /// (§7.14f — the defect this split repairs).
    ///
    /// [`Self::open_preview_image_on`]'s argument, word for word, now made true
    /// of the third lane as well: *the pane you aimed at is the pane that takes
    /// the file, whether or not it is the one an ordinary open would have
    /// chosen.* The page lane used to drop its caller's surface on the floor and
    /// re-ask the landing rule, so a `.html` dropped on a seat of its own opened
    /// in whichever preview pane happened to be first in tree order — the pane
    /// the reader was reading — and the seat the drop had just minted stayed the
    /// empty placeholder.
    ///
    /// The four steps of the entry are all here and their order is the whole
    /// rule:
    ///
    /// 1. **The disk says what the path is.** `canonicalize` resolves the
    ///    junctions, the `..`s, the short names and the case, and what comes back
    ///    is the only spelling anything downstream sees. A path that is not there
    ///    is not a page: it earns the disk's own sentence, on **this** surface.
    /// 2. **The host mints.** `webnav::Mint::file` turns that `PathBuf` into the
    ///    one `file:` URL this seat may load — percent-encoding the four
    ///    characters that would re-open the parse, and refusing a network path
    ///    outright. **No string from anywhere is trusted**: the URL is built from
    ///    the canonicalised path, never from an address, a row or a session file.
    /// 3. **The host asks its own gate** (`webnav::Origin::HostMinted`), which is
    ///    [`Self::open_minted_page_on`]'s first line.
    /// 4. **The mint travels with the request** to `WebSeat`, which installs it
    ///    before it calls `Navigate` — because `NavigationStarting` can fire
    ///    before `Navigate` returns, and a gate asked about a target the pane has
    ///    not yet admitted to minting would cancel the pane's own navigation.
    ///
    /// Both refusals are landed on the caller's surface for the reason §7.39
    /// gave for the float's: the card belongs where the reader is looking, which
    /// is where they aimed.
    fn open_preview_web_file_on(&mut self, surface: PreviewSurface, path: PathBuf) -> Result<()> {
        let canonical = match std::fs::canonicalize(&path) {
            Ok(canonical) => canonical,
            Err(error) => {
                self.mouse_trace(|| format!("open_preview_web_file leave=no-disk error={error}"));
                // "一个不在那里的路径不是一张网页" (§7.10 ①), and the card it
                // earns is the disk's own sentence — which this door is holding
                // and the document lane is not (user ruling 2026-08-23).
                return self.land_page_refusal_on(
                    surface,
                    path,
                    preview::PreviewRefusal::Fault(preview::PreviewFault::from_io(&error)),
                );
            }
        };
        let mint = match webnav::Mint::file(&canonical) {
            Ok(mint) => mint,
            Err(refusal) => {
                // A share, reached through a canonicalised path that turned out
                // to be one — a junction can make a local-looking path into a
                // UNC one, which is why this is asked after `canonicalize` and
                // not only at [`preview_open_lane`]. `NetworkPath` is section
                // 7.1.3's own refusal and the one this window has always shown.
                self.mouse_trace(|| format!("open_preview_web_file leave=refused {refusal:?}"));
                return self.land_page_refusal_on(
                    surface,
                    path,
                    preview::PreviewRefusal::NetworkPath,
                );
            }
        };
        match surface {
            PreviewSurface::Seat(leaf) => self.open_minted_page_on(leaf, mint),
            PreviewSurface::Float(id) => self.open_minted_page_on_float(id, mint),
            // **A glance card has no engine and never will** (§7.14a: a card's
            // life is a few hundred milliseconds and a browser process is not).
            // It never arrives here — the card fills its own buffer and does not
            // come through the pool's landing door — and the honest answer for a
            // surface that cannot host a browser is to open none, said once
            // rather than guarded at every caller.
            PreviewSurface::Peek => {
                self.mouse_trace(|| "open_preview_web_file leave=peek-hosts-no-engine".to_owned());
                Ok(())
            }
        }
    }

    /// **Play the video this surface is showing** (user ruling 2026-08-28,
    /// route B slice ②; `docs/DESIGN.md` §7.44 ①).
    ///
    /// **The one verb, for all three surfaces**, which is the whole ruling in a
    /// signature: the parameter is a [`PreviewSurface`], nothing below this line
    /// asks which kind it is, and a pane, a floating window and a glance card
    /// therefore start a video the same way by construction rather than by three
    /// call sites remembering to agree.
    ///
    /// Route A wrote a page into a cache folder, minted a `file:` URL for it and
    /// navigated a browser at it (§7.23 ⑩). All of that is gone: what happens
    /// here is that Media Foundation opens the recording on a thread of its own
    /// and this window draws the frames (§7.42). The four ways it declined are
    /// down to three, and the one that went was the shell.
    ///
    /// 1. **No video on it**, or a name outside [`preview::path_names_a_video`].
    ///    This is the *same* predicate [`Self::seats_wearing_a_play_button`]
    ///    draws by, so a button that exists is a button that works.
    /// 2. **Already playing**, which is a play pressed twice and is a play.
    /// 3. **The decoder refused it** — a Store codec that is not installed, a
    ///    file that is not what its name says. Unlike route A this is *not*
    ///    silent: the seat is not created, and the surface says so out of
    ///    [`video_seat::VideoSeat::fault`]'s own error, which is the honest
    ///    source of that sentence now that no compiled table can know it.
    ///
    /// **No canonicalisation.** Route A needed the disk's own spelling because a
    /// URL had to be minted from it; a decoder is handed a path and opens it, so
    /// the spelling this window knows the file by is the only one there is —
    /// which retires a whole class of defect (§7.23 ⑩'s two-spellings bug) by
    /// retiring the second spelling.
    pub(crate) fn play_video_on(&mut self, surface: PreviewSurface) -> Result<()> {
        let Some(path) = self
            .preview_picture(surface)
            .map(|picture| picture.path.clone())
        else {
            return Ok(());
        };
        self.play_video_file_on(surface, &path)
    }

    /// The same, on a path the caller already has — the door a glance card comes
    /// through, since a card's recording is not a `PreviewImageState`.
    fn play_video_file_on(&mut self, surface: PreviewSurface, path: &Path) -> Result<()> {
        if !preview::path_names_a_video(path) {
            return Ok(());
        }
        if self.window.video.get(surface).is_some() {
            return Ok(());
        }
        if let Err(error) = self.window.video.open(surface, path, Instant::now()) {
            self.mouse_trace(|| format!("play_video_on leave=no-engine {error:?}"));
            // **And the surface says so** (§7.44 ⑥). This is the honest source
            // of `Text::VideoFormatCannotPlay`, which used to be printed off a
            // column of a compiled table and could therefore never know what
            // *this machine* has: an HEVC `.mp4` or a VP9 `.webm` plays where
            // the Store codec is installed and does not where it is not, and the
            // only thing that knows which is the decoder that just refused.
            //
            // Filed on the `PreviewImageState`, which is the one place a
            // surface's failures already live and already draw — the same slot a
            // picture that would not decode writes into.
            if let Some(picture) = self.preview_picture_mut(surface) {
                picture.failure = Some(PictureRefusal::this_window_cannot(
                    i18n::Text::VideoFormatCannotPlay.text(),
                ));
            }
        } else if let Some(picture) = self.preview_picture_mut(surface) {
            // And a play that worked clears whatever the last one said.
            picture.failure = None;
        }
        // The still comes off the glass and the layer goes on in the same pass,
        // which is the pass that already owns both — see
        // [`Self::refit_preview_picture`].
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// **Stop the video this surface is playing and put its frame back** (user
    /// ruling 2026-08-28; §7.44 ①).
    ///
    /// [`Self::play_video_on`]'s exact undo, and it is short for the reason
    /// route A's was: the surface never stopped being the recording's surface,
    /// so there is nothing to restore. The `PreviewImageState` is where it was,
    /// the decoded first frame is still in the window's cache under the same
    /// key, and the breadcrumb never changed.
    ///
    /// **The engine is shut down and not paused**, which is the whole difference
    /// between this and switching tabs: a video on a tab nobody is looking at
    /// goes on playing by the same ruling that grew the tab a speaker, and a
    /// stopped one has no decoder left to play with. Shutting down is
    /// idempotent and `Drop` does it too (§7.42 ⑦), so a surface that is torn
    /// away rather than stopped does not leak one either.
    ///
    /// Silent on a surface with nothing playing: the tool is only drawn over one
    /// that has something, and a stop pressed twice is a stop.
    pub(crate) fn stop_video_on(&mut self, surface: PreviewSurface) -> Result<()> {
        if !self.window.video.close(surface) {
            return Ok(());
        }
        // The frame comes back on the next fit, which is this frame: the pixels
        // never left the cache and `refit_preview_picture`'s refusal is lifted
        // by the same predicate that put it there.
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// [`Self::open_preview_file`] on a surface the caller has already chosen —
    /// see [`Self::open_preview_image_on`] for why the two are separable.
    pub(crate) fn open_preview_file_on(
        &mut self,
        surface: PreviewSurface,
        path: PathBuf,
    ) -> Result<()> {
        let name = files_row_display_name(&path);
        self.open_preview_source_on(surface, preview::PreviewSource::file(path), name)
    }

    /// **The pool's own door, told an identity rather than a path** (G-0).
    ///
    /// [`Self::open_preview_file_on`] is this with the one step a *file* needs
    /// in front of it — deriving the display name from the path — and it is the
    /// only step that could not be written for a document that has no path. The
    /// switcher already comes through here with a name it read off the pool
    /// rather than off a filesystem, and G-1's git surfaces will come the same
    /// way: everything below this line is about a buffer and a view of it, and
    /// nothing below it asks where the bytes are.
    pub(crate) fn open_preview_source_on(
        &mut self,
        surface: PreviewSurface,
        source: preview::PreviewSource,
        name: String,
    ) -> Result<()> {
        // **A name that says page opens as a page, whichever door it came
        // through** (user ruling 2026-08-23; §7.10 ⑥). This is the pool's own
        // door, so it is the door every *document* arrives by — a drop, a
        // switcher row, a Recent seed, a restored `cur`, a double click in the
        // files column — and putting the fork here rather than at each of them
        // is what makes "从任何入口进来都开成渲染页" true by construction
        // instead of by six call sites remembering.
        //
        // **And it opens on the surface it was landed on** (§7.14f, user report
        // 2026-09-06). This used to read "the seat is the singleton rule's
        // rather than the one the caller chose", which was the last living piece
        // of the per-tab singleton §7.14e ② retired: the arm threw the caller's
        // surface away and re-asked the landing rule, so a page dropped on a
        // seat of its own opened in the *reader's* preview pane — clearing what
        // was in it — while the seat the drop had minted stayed empty. A page is
        // a preview buffer (§7.9), so where it lands is not the page lane's
        // question at all; every door above already answered it, and a float
        // (§7.39) is only the second arm of the same sentence.
        if let Some(path) = source_opens_as_a_page(&source) {
            return self.open_preview_web_file_on(surface, path);
        }
        self.land_preview_source_on(surface, source, name)
    }

    /// [`Self::open_preview_source_on`] with the fork already answered — **the
    /// document lane itself**.
    ///
    /// One caller other than that door: [`Self::land_page_refusal`], which is
    /// the arm where the page lane has *already* asked the disk and been
    /// refused. Sending it back through the fork would be an infinite loop, and
    /// the loop is the honest shape of the thing being avoided — the page lane
    /// and the document lane each believing the other one has the answer.
    pub(crate) fn land_preview_source_on(
        &mut self,
        surface: PreviewSurface,
        source: preview::PreviewSource,
        name: String,
    ) -> Result<()> {
        // **The second of the ruling's two moments** (2026-08-29): a document
        // is being brought to the front. Asked *before* the pool is opened, so
        // that a buffer already in it is judged against the disk as it stands
        // now rather than as it stood when it left the glass — which is the very
        // gesture the reported defect was found by.
        self.ask_the_unwatched_preview_files()?;
        // The picture on this surface, if there was one, is not what it is
        // showing any more. Cleared before the buffer lands so no frame between
        // the two can find both.
        self.clear_preview_image(surface);
        let tab = self.id;
        let shown = self.preview_panes.showing();
        let buffer = self.preview_pool.open(source.clone(), name, &shown);
        // Claimed rather than merely asked about: every send on this channel
        // goes through the one door, so a file already out with the worker —
        // opened into a second pane, or armed by the card column a frame ago —
        // is read once (user ruling 2026-08-21).
        let wants_read = buffer.claim_head_read();
        // The outgoing view is filed and the incoming one is found: a caret and
        // a scroll are the pane's memory of a file, and a switch that reset them
        // would make the switcher a thing you pay for using (ruling 8⑧).
        self.leave_preview_buffer(surface);
        let view = self.preview_views.restore(&source);
        let pane = self.preview_pane_mut(surface);
        pane.buffer = Some(source.clone());
        pane.caret = view.caret;
        pane.scroll = view.scroll;
        if let Some(want) = wants_read
            && !self.app.preview_worker.request(preview::PreviewRequest {
                window: self.window_id(),
                tab,
                source,
                want,
            })
        {
            self.disable_preview_worker();
        }
        // `cur` on disk is which file each pane was reading (slice 7): a switch
        // that never marked the session dirty shipped yesterday's answer — the
        // restored pane reopened the file you had already moved on from
        // (found during G-0's real-machine pass, pre-existing).
        self.mark_session_dirty(Instant::now());
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// The pane stopped showing a document. **The buffer does not go away.**
    ///
    /// The pool is the tab's and outlives any one pane (§7.1.3): what a closing
    /// pane takes with it is the *view*, which is which buffer it was on and how
    /// far down. The pool itself is emptied only by the dirty gates, which are
    /// slice 4's — and until edits exist there is nothing dirty for them to
    /// guard, so nothing here has anything to confirm.
    pub(crate) fn clear_preview_view(&mut self, surface: PreviewSurface) {
        self.leave_preview_buffer(surface);
        self.refresh_preview_body();
    }

    /// Take this surface's picture down.
    ///
    /// The pixels go with it because the picture channel is rebuilt from the
    /// panes on the next refit ([`Self::refresh_preview_for_layout`]) and a pane
    /// with no `image` contributes nothing to it. There is no lane to release
    /// and therefore no way for one preview closing to blank another's, which is
    /// exactly what the release this replaced had to be careful about.
    pub(crate) fn clear_preview_image(&mut self, surface: PreviewSurface) {
        self.preview_pane_mut(surface).image = None;
    }

    /// The same, told **which tab** the surface's view lives in — the twin of
    /// [`Self::leave_preview_buffer_in`], and it exists for that function's own
    /// reason.
    ///
    /// A page opening on a tab that is not in front is the caller
    /// ([`Self::open_web_page_on`], since F1b′): `preview_pane_mut` resolves a
    /// seat number against the first tab whose tree holds it, so it answers for a
    /// tab that is not this page's when two tabs number a preview seat the same.
    pub(crate) fn clear_preview_image_in(&mut self, index: usize, surface: PreviewSurface) {
        self.window.tabs[index].preview_panes.entry(surface).image = None;
    }

    /// This surface is about to stop showing whatever it is showing. **File the
    /// view, keep the buffer.**
    ///
    /// One door for the three ways a pane leaves a document — another file, a
    /// picture, the pane closing — because the thing that must not be forgotten
    /// is the same in all three, and a caret remembered in two of them is a
    /// caret that goes missing depending on how you left.
    ///
    /// The tab's singletons — the hand on a thumb, the hover, the keyboard, the
    /// selection being drawn — are cleared **only when they name this surface**.
    /// There is one pointer and one keyboard, so a pane leaving a document has no
    /// business answering for a gesture that is happening in the pane beside it.
    fn leave_preview_buffer(&mut self, surface: PreviewSurface) {
        let index = self.preview_tab_index(surface);
        self.leave_preview_buffer_in(index, surface);
    }

    /// The same, told **which tab** the surface's view lives in.
    ///
    /// The sweep is the one caller that has to say: it runs after the window has
    /// already left the host, so [`Self::preview_tab_index`] can no longer be
    /// asked whose the view was, and filing a torn-off window's caret into
    /// whichever tab happened to be on screen is how a scroll position ends up
    /// remembered against the wrong file.
    pub(crate) fn leave_preview_buffer_in(&mut self, index: usize, surface: PreviewSurface) {
        // A surface that has never shown anything has no view to file, and
        // vivifying one here only to empty it would leave a pane behind for the
        // sweep to retire a moment later.
        if self.window.tabs[index].preview_panes.get(surface).is_some() {
            let pane = self.window.tabs[index].preview_panes.entry(surface);
            let left = pane.buffer.take();
            let view = PreviewViewState {
                caret: pane.caret,
                scroll: pane.scroll,
            };
            pane.caret = preview_edit::EditCaret::default();
            pane.scroll = [0.0, 0.0];
            // The surface is being emptied, not re-parsed: there is nothing left
            // for a mark to be about, so this is the arm that clears.
            pane.show_document(PreviewDocument::Empty, Reparse::Elsewhere);
            pane.doc_key = None;
            // The block offsets this document was scrolled by mean nothing to
            // the next one.
            pane.md_block_scroll.clear();
            // The next document's links are measured with its body; what a hand
            // was resting on in this one is not a fact about that one, and a
            // stale box would keep the pointing finger over prose that answers
            // nothing.
            pane.links.clear();
            pane.notice = None;
            // **The caret is not standing on the next document's page** (T5,
            // §7.1.3t). Filed with the caret it belongs to and taken off here
            // with it: a surface that comes back to this file brings back the
            // byte offset it was left at, and a surface handed a different file
            // opens it as a page to read rather than as one being typed into.
            // The pending press goes for the plainer reason — the body it was
            // waiting for is not the body that is coming.
            pane.md_caret = false;
            pane.md_caret_wanted = None;
            if let Some(source) = left {
                // The memory is per buffer and the buffer is the tab's, so it is
                // filed in the same tab the view came out of.
                self.window.tabs[index]
                    .preview_views
                    .remember(&source, view);
            }
        }
        if self.preview_edit_focus == Some(surface) {
            self.preview_edit_focus = None;
        }
        if self.preview_selecting == Some(surface) {
            self.preview_selecting = None;
        }
        // A selection being drawn across a page that has just been swapped out
        // is a selection of a document nobody is looking at any more.
        if self
            .preview_text_drag
            .as_ref()
            .is_some_and(|drag| drag.surface == surface)
        {
            self.preview_text_drag = None;
        }
        // A thumb held over the swap would be dragging a block that is not there
        // any more.
        if self
            .preview_block_drag
            .is_some_and(|drag| drag.surface == surface)
        {
            self.preview_block_drag = None;
        }
        if self
            .preview_block_hover
            .is_some_and(|(hovered, _)| hovered == surface)
        {
            self.preview_block_hover = None;
        }
        // And the body's own bar with them: the next document is a different
        // length, so a thumb held across the swap would be dragging a track that
        // means something else.
        if self
            .preview_body_drag
            .is_some_and(|drag| drag.surface == surface)
        {
            self.preview_body_drag = None;
        }
        if self
            .preview_body_hover
            .is_some_and(|(hovered, _)| hovered == surface)
        {
            self.preview_body_hover = None;
        }
        if self
            .preview_link_hover
            .as_ref()
            .is_some_and(|(hovered, _)| *hovered == surface)
        {
            self.preview_link_hover = None;
        }
    }

    /// What **this seat's** preview head is showing this frame, with both of its
    /// strings measured.
    ///
    /// **The measurement is stored, not returned twice.** The hit test has to
    /// agree with the paint about where the switcher's pill ends, and it is
    /// `&self` by construction — a pointer moving is not a reason to touch a
    /// renderer — so the two widths land in the runtime here, exactly as the
    /// files heads' do. Those two widths are still a *singleton* cache, which is
    /// the one place this slice stops: the chrome layer draws one preview head,
    /// `refresh_chrome` asks it about `seats.preview()`, and drawing a head per
    /// preview leaf is a step of its own.
    pub(crate) fn dress_preview_head(
        &mut self,
        seat: SeatId,
        scale: f32,
    ) -> Option<PreviewHeadFrame> {
        let surface = self.preview_here(seat);
        // A picture has a name and no buffer, and it still gets a head: the two
        // doors into this seat fill the same caption (P36's contract), and a head
        // that appeared only for documents would blink out every time you looked
        // at a `.png`.
        let md_source = self.preview_md_source(surface);
        // **A page on this seat answers for its own head** (§7.7 ②). Its name is
        // the document's title and its verbs are the three buttons, so it is
        // asked before the buffer: this build's page does not live in the pool
        // yet (slice ③), and a seat with a page and no buffer would otherwise
        // draw an empty caption over a live document.
        let page = self.web_on(seat).map(|web| web.page().clone());
        let (name, tools, dirty, flip_to_source) = match self.preview_buffer_on(surface) {
            Some(buffer) => (
                buffer.name.clone(),
                seats::PreviewHeadTools {
                    save: buffer.is_editable(md_source),
                    // **Only where no rail can carry it** (user ruling
                    // 2026-08-24). The flip moved down to the breadcrumb row
                    // with the other verbs that are about the file; a document
                    // with no path grows no breadcrumb, and its flip stays here
                    // rather than vanishing with the row it would have stood in.
                    flip: buffer.ftype == preview::PreviewFtype::Markdown
                        && self.preview_rail_kind(self.preview_here(seat)).is_none(),
                    ..seats::PreviewHeadTools::default()
                },
                buffer.dirty,
                // The glyph names the *destination*, and the destination is this
                // surface's own: a pane showing the render offers "edit source"
                // while a float on the same file, already flipped, offers "eye".
                !md_source,
            ),
            None => (
                self.preview_pane(surface)
                    .and_then(|pane| pane.image.as_ref())
                    .map(PreviewImageState::title)
                    .unwrap_or_default(),
                seats::PreviewHeadTools::default(),
                false,
                false,
            ),
        };
        // **What a page's head says, over whatever the buffer said.** The name
        // cell is the document's title — and the address until a document has
        // said what it is called, because a blank caption over a page that is
        // loading would be the one moment this head says nothing at all. The
        // dirty slot stays structurally present and structurally empty: a page
        // has no unsaved text, and an empty cell is what this window looks like
        // when it has no ledger to speak from (§7.1.6h, the same sentence).
        // A seat whose one navigation was refused has no title and no committed
        // address, and the head must still name what it was handed — otherwise
        // the one cell that says what this seat is about is blank while the card
        // under it names the address in full.
        let (name, tools, dirty, flip_to_source) = match &page {
            Some(page) => (
                // **The title, and only the title** (user ruling 2026-08-24).
                // This cell used to fall back to the address, on the argument
                // that a blank caption over a loading page would be the one
                // moment the head said nothing at all — and while the name *was*
                // the address that fallback was the head being honest. The
                // address has its own row now, filled from the same two facts in
                // the same order, so a head that fell back would be this pane
                // printing one string twice. What is left when a document has
                // not said what it is called is the address it was handed, which
                // is the row below saying it; and a seat whose one navigation
                // was refused has neither, which is the state the card in its
                // body is already about.
                // **A player's head says the recording and not the shell**
                // (user ruling 2026-08-27; §7.23 ⑩ — found on the machine,
                // where the head over a playing video read `Folio player`).
                // The document's title is the shell's, and the shell is this
                // window's page rather than the reader's document: the file
                // they opened is the recording, and this is the one cell that
                // says what this seat is about.
                match self.video_playing_on(surface) {
                    Some(video) => files_row_display_name(video),
                    None => page.title.clone(),
                },
                seats::PreviewHeadTools {
                    save: false,
                    flip: false,
                    // The per-type slot's other occupant — see
                    // [`Self::preview_head_tools`], which is what the hit test
                    // reads and which this must agree with to the pixel.
                    stop: self.surface_is_playing_a_video(surface),
                    // **The developer tools, and nothing else of a page's four**
                    // (user ruling 2026-08-24, second round). The hand-off arrow
                    // that this comment used to defend went down to the address
                    // row with `‹ › ⟳`: 「在浏览器打开」 is a verb about an
                    // address, and §7.1.5g ②″'s reason travels with it rather
                    // than being spent twice.
                    web: true,
                    ..tools
                },
                false,
                false,
            ),
            None => (name, tools, dirty, flip_to_source),
        };
        let pool = self.preview_pool.len();
        // `othersDirty` (P19): the pane's own dot already speaks for the buffer on
        // screen, so the badge is the only thing that can speak for the rest.
        let shown = self
            .preview_pane(surface)
            .and_then(|pane| pane.buffer.clone());
        let others_dirty = self
            .preview_pool
            .dirty_names(shown.as_ref())
            .next()
            .is_some();
        let count = if pool > 1 {
            pool.to_string()
        } else {
            String::new()
        };
        let tools = seats::PreviewHeadTools {
            switcher: self.preview_switcher_rows(seat) > 1,
            // Read off the seat and not off the buffer, which is what makes it
            // survive the two `match`es above: a lock is a fact about the pane,
            // and the pane keeps it whatever it is showing.
            locked: self.seats.preview_is_locked(seat),
            ..tools
        };
        // **Measured wearing what they are drawn wearing** (user report,
        // 2026-08-25). Both strings used to be measured with
        // `measure_chrome_text`, which is the face's regular weight by
        // definition, while the head draws the name Medium and the badge Medium
        // in tabular figures — so the name's box came out a few pixels short of
        // the name and the clip cut the last glyph in half. The faces are
        // `seats`' own declaration now, read by the paint as well as by this.
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32, face: git_panel::MeasureFace| {
            renderer.measure_chrome_label(
                gpu,
                text,
                size,
                face.weight,
                face.letter_spacing_em,
                face.tabular_numerals,
            )
        };
        let tools = seats::preview_head_measurements(&name, &count, scale, tools, &mut measure);
        // **The editor, if this head is the one holding it** (user ruling
        // 2026-08-19). It comes last because it *replaces* the name's measured
        // width with the draft's: the box is sized to what is being typed, and
        // the hit test has to be handed the same number the paint used, which
        // is what the store below is for.
        let (tools, edit) = self.dress_preview_name_editor(seat, surface, scale, tools);
        let here = self.leaf_here(seat);
        self.window.preview_head_measures.insert(here, tools);
        Some(PreviewHeadFrame {
            name,
            count,
            edit,
            // **A file's name has no refusal** and the address's moved downstairs
            // with the field: this head's editor is the rename editor now, and a
            // rename that will not go through says so by leaving the name as it
            // was (`commit_preview_rename`'s own ruling) rather than by turning
            // the draft red.
            refused: false,
            content: seats::PreviewHeadContent {
                tools,
                dirty,
                others_dirty,
                flip_to_source,
                menu_open: self.preview_menu_seat() == Some(seat),
                ..seats::PreviewHeadContent::default()
            },
        })
    }

    /// **The file one preview seat is standing on**, when it is standing on one.
    ///
    /// The predicate behind 「面包屑只长在有磁盘路径的预览上」 (user ruling
    /// 2026-08-24), and it is the pair [`Self::reveal_preview_file`] already
    /// asks: a buffer's own path, or a picture's. A composed document — a diff,
    /// a commit's reading of a file, a graph — answers `None`, and a seat that
    /// answers `None` grows no breadcrumb.
    pub(crate) fn preview_rail_path(&self, surface: PreviewSurface) -> Option<PathBuf> {
        self.preview_buffer_on(surface)
            .and_then(|buffer| buffer.source.file_path().map(Path::to_path_buf))
            .or_else(|| {
                self.preview_pane(surface)
                    .and_then(|pane| pane.image.as_ref())
                    .map(|image| image.path.clone())
            })
    }

    /// Which row this preview seat wears under its head, if any.
    ///
    /// **A page first**, and the order is the ruling's: 「网页不长面包屑」. A
    /// local `.html` rendered as a page has a disk path *and* an address, and it
    /// is the address that the reader is navigating with — the path is still one
    /// press away, in the `Open ⌄` the address row does not carry, which is the
    /// trade the ruling made when it said a page's row is its address.
    /// **A player is not a page here, and this is the line that says so** (user
    /// ruling 2026-08-27; §7.23 ⑩). The engine on a playing video's pane is
    /// hosting a shell this window wrote into a cache folder — a document nobody
    /// asked for, at a path nobody would recognise — and the file the reader
    /// opened is the recording. So the row it wears is the recording's
    /// breadcrumb, exactly as it was a moment before the play button was
    /// pressed, and pressing play does not change what this pane is *about*.
    ///
    /// Said here rather than in [`Self::rail_page`], which every placement,
    /// keyboard and retirement question in this file also asks: the engine is
    /// genuinely on this pane and every one of those must go on knowing it. What
    /// changes is only what the pane *says it is*.
    pub(crate) fn preview_rail_kind(
        &self,
        surface: PreviewSurface,
    ) -> Option<seats::PreviewRailKind> {
        if self.rail_page(surface).is_some() && !self.surface_is_playing_a_video(surface) {
            return Some(seats::PreviewRailKind::Address);
        }
        self.preview_rail_path(surface)
            .map(|_| seats::PreviewRailKind::Crumbs)
    }

    /// **The recording a surface is playing**, and `None` for a surface that is
    /// playing nothing (user ruling 2026-08-28; §7.44 ①).
    ///
    /// One map, asked of a *surface*, which is the form every caller in this
    /// file wants: the rail, the frame, `↗` and the press are all about a
    /// surface and none of them holds a leaf. Route A had to ask a browser what
    /// it was showing and the browser had to ask its mint, because the thing on
    /// the pane was a page in a cache folder and the recording's name was
    /// written on the side of it. There is no page now, so the question is one
    /// lookup: a surface with a seat is playing the file that seat was opened
    /// on, and there is nowhere for a second answer to come from.
    fn video_playing_on(&self, surface: PreviewSurface) -> Option<&Path> {
        Some(self.window.video.get(surface)?.path())
    }

    /// **What one surface is showing**, asked of every lane that can fill it —
    /// [`surface_subject_of`] with this window's two questions put to it
    /// (§7.44 ⑬).
    fn preview_subject(&self, surface: PreviewSurface) -> SurfaceSubject {
        surface_subject_of(self.preview_pane(surface), self.web_of(surface).is_some())
    }

    /// The same as a yes or no, for the callers that only need the fork.
    pub(crate) fn surface_is_playing_a_video(&self, surface: PreviewSurface) -> bool {
        self.video_playing_on(surface).is_some()
    }

    /// Recompute which seats wear a rail and tell the tree, re-solving when the
    /// answer moved a rectangle.
    ///
    /// [`Runtime::settle_pane_notices`]'s shape and its contract, for its
    /// reason: the row changes what a pane's body is, so a change here is a
    /// layout change and everything downstream of a rectangle has to follow.
    pub(crate) fn settle_preview_rails(&mut self) -> Result<()> {
        let rails: BTreeMap<SeatId, seats::PreviewRailKind> = self
            .seats
            .preview_seats()
            .into_iter()
            .filter_map(|seat| Some((seat, self.preview_rail_kind(self.preview_here(seat))?)))
            .collect();
        if !self.seats.set_rails(rails) {
            return Ok(());
        }
        // The same three steps a notice's arrival takes, and for its reason: the
        // row changes the pane's height, so every rectangle downstream of it —
        // the shell's rows, the page's bounds, the wheel's reach — has to be
        // recomputed before anything is drawn against them.
        self.commit_seat_geometry()?;
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// **One preview rail, dressed** (user ruling 2026-08-24) —
    /// [`Self::dress_preview_head`]'s opposite number.
    ///
    /// Everything the row needs, measured where a font is: the address or the
    /// segments, the widths the geometry has to be laid out to, and the open
    /// editor when this rail is the one holding it. A seat with no rail answers
    /// `None`, and the paint draws nothing rather than an empty band.
    pub(crate) fn dress_preview_rail(
        &mut self,
        surface: PreviewSurface,
        scale: f32,
    ) -> Option<PreviewRailFrame> {
        let kind = self.preview_rail_kind(surface)?;
        let font = seats::PREVIEW_RAIL_FONT_LOGICAL_PX * scale;
        let mut frame = PreviewRailFrame {
            measure: seats::PreviewRailMeasure {
                kind,
                ..seats::PreviewRailMeasure::default()
            },
            address: String::new(),
            segments: Vec::new(),
            targets: Vec::new(),
            meta: String::new(),
            edit: None,
            refused: false,
            flip_to_source: false,
            web: seats::WebHeadState::default(),
        };
        match kind {
            seats::PreviewRailKind::Address => {
                let page = self.web_of(surface).map(|web| web.page().clone())?;
                // **The committed address, and the refused one when a seat's one
                // navigation was turned away** — the same pair the head's name
                // cell used to fall back through, arriving where it belongs now
                // that the name is a title again. A blank row over a page that
                // was handed an address it would not go to would be this window
                // forgetting what it was asked for.
                let refused_address = self
                    .web_of(surface)
                    .and_then(webhost::WebSeat::fault)
                    .and_then(webhost::WebFault::refused_address);
                frame.address = shown_address(&if page.url.is_empty() {
                    refused_address.unwrap_or_default()
                } else {
                    page.url.clone()
                });
                frame.web = seats::WebHeadState {
                    can_go_back: page.can_go_back,
                    can_go_forward: page.can_go_forward,
                    loading: page.loading,
                };
                frame.measure.address_width = self.window.renderer.measure_chrome_text(
                    &mut self.app.gpu,
                    &frame.address,
                    font,
                );
                // **`</>` on a page's row too** (user ruling 2026-08-26; DESIGN
                // §7.7 ⑭). The offer and the state are two questions and both
                // are asked here: whether this page has a file on this disk that
                // this window can read, and which of its two faces is up. The
                // developer tools are untouched and stay on the head — a debugger
                // and a reading are different errands, which is the ruling's own
                // sentence.
                frame.measure.flip = self.preview_page_source_file(surface).is_some();
                // The glyph names the *destination*, exactly as it does one arm
                // down.
                frame.flip_to_source = self.page_source_shown_on(surface).is_none();
            }
            seats::PreviewRailKind::Crumbs => {
                let path = self.preview_rail_path(surface)?;
                // **The draft stands in for the tail while a box is open on it**
                // (B5, user ruling 2026-08-25). The whole row is laid out to the
                // widths measured here, so substituting the draft *before* the
                // measurement is what makes the box grow and shrink under the
                // typing instead of the letters running out of a rectangle cut
                // for the old name. Every other segment is untouched: the
                // folders above the file have not moved.
                let drafted = self.open_crumb_draft(surface);
                let last = crumb_segments(&path).len().saturating_sub(1);
                for (at, (name, target)) in crumb_segments(&path).into_iter().enumerate() {
                    let name = match &drafted {
                        Some(draft) if at == last => draft.clone(),
                        _ => name,
                    };
                    frame
                        .measure
                        .segments
                        .push(self.window.renderer.measure_chrome_text(
                            &mut self.app.gpu,
                            &name,
                            font,
                        ));
                    frame.segments.push(name);
                    frame.targets.push(target);
                }
                let md_source = self.preview_md_source(surface);
                frame.measure.flip = self
                    .preview_buffer_on(surface)
                    .is_some_and(|buffer| buffer.ftype == preview::PreviewFtype::Markdown);
                // The glyph names the *destination*, which is the head's rule
                // travelling down with the button it belongs to.
                frame.flip_to_source = !md_source;
                frame.measure.open_width = self.window.renderer.measure_chrome_text(
                    &mut self.app.gpu,
                    i18n::Text::PreviewRailOpen.text(),
                    font,
                );
                // **The standing fact, worn as a padlock** (owner's ruling
                // 2026-09-12). One question — is this surface owed a fact that
                // is true for as long as you are looking at it — asked once here
                // for both hosts, so a pane and the window torn off it cannot
                // come to say two different things about one file. The sentence
                // itself is never laid out on this row: it is the lock's tip,
                // and it is said in words only on the pill that answers a
                // refused edit.
                frame.measure.lock = self
                    .preview_standing_fact(surface, Instant::now())
                    .is_some();
                // **And what the picture is, dim, inboard of it** — the line
                // that used to stand under the photograph (mock-up 4955).
                frame.meta = self
                    .preview_meta_sentence(surface, scale)
                    .unwrap_or_default();
                frame.measure.meta_width = if frame.meta.is_empty() {
                    0.0
                } else {
                    self.window.renderer.measure_chrome_text(
                        &mut self.app.gpu,
                        &frame.meta,
                        seats::FILES_FOOT_FONT_LOGICAL_PX * scale,
                    )
                };
            }
        }
        let (tools, edit) =
            self.dress_preview_address_editor(surface, scale, frame.measure.clone());
        frame.measure = tools;
        frame.edit = edit;
        // **And the tail's box, when this rail is the one holding it** (B5). The
        // two are mutually exclusive by construction — an `Address` rail has no
        // crumbs and a `Crumbs` rail has no address — so this cannot overwrite an
        // open address field; it is written second because the measurement it
        // reads is the one the line above settled.
        if frame.edit.is_none() {
            frame.edit = self.dress_preview_crumb_editor(surface, scale, &frame.measure);
        }
        // The same door the commit goes through, asked without knocking — an
        // empty field is unfinished rather than wrong, so it does not light up.
        // **The engine the search would be composed for is handed over** so that
        // this is not merely the same rule as the commit but the same call: a
        // predicate that judged a phrase against a different engine's template
        // than the one Enter would use is a second door wearing the first one's
        // name.
        let engine = self.app.settings_store.loaded().search_engine;
        frame.refused = matches!(
            self.window.rename.as_ref().map(|editor| &editor.subject),
            Some(RenameSubject::WebAddress { .. })
        ) && !self
            .window
            .rename
            .as_ref()
            .is_some_and(|editor| webhost::WebSeat::would_go_to(editor.text(), engine));
        self.window
            .preview_rail_measures
            .insert(surface, frame.measure.clone());
        Some(frame)
    }

    /// The measurement **this seat's** rail must be laid out with — this
    /// frame's, off the widths the paint stored for that seat.
    ///
    /// [`Self::preview_head_tools`]'s twin, and it answers the same way for the
    /// same reason: the hit test is `&self` by construction and cannot measure,
    /// so it reads the number the picture was built from. A seat whose rail has
    /// not been drawn yet has no entry, and a row that has never been drawn has
    /// nothing to press.
    pub(crate) fn preview_rail_measure(
        &self,
        surface: PreviewSurface,
    ) -> Option<seats::PreviewRailMeasure> {
        self.window.preview_rail_measures.get(&surface).cloned()
    }

    /// **Every control of every preview rail, registered for a tip** (user
    /// ruling 2026-08-25).
    ///
    /// The head's own pass one band lower, and built the same way: the boxes are
    /// the paint's own ([`seats::preview_rail_tip_boxes`] off the very geometry
    /// the hit test reads), and the words are this window's, because two of the
    /// five are a *path* rather than a phrase.
    pub(crate) fn preview_rail_tip_anchors(&mut self, anchors: &mut tooltip::TooltipAnchors) {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // **Every rail on the glass and not only the tree's** (§7.7 ⑩ 欠账,
        // 2026-08-25). A `⧉` does not stop needing a word for what it copies
        // because the pane it stood in was torn into a window, and the anchor id
        // says so by carrying the surface — [`tooltip::TooltipAnchorId::PreviewRail`].
        let surfaces: Vec<PreviewSurface> = self
            .seats
            .preview_seats()
            .into_iter()
            .map(|seat| self.preview_here(seat))
            .chain(
                self.window
                    .float
                    .live_windows()
                    .filter(|win| win.preview().is_some())
                    .map(|win| PreviewSurface::Float(win.epoch)),
            )
            .collect();
        for surface in surfaces {
            let Some(measure) = self.preview_rail_measure(surface) else {
                continue;
            };
            let Some(geometry) = self.rail_geometry(surface, scale) else {
                continue;
            };
            let segments = self
                .preview_rail_path(surface)
                .map(|path| crumb_segments(&path))
                .unwrap_or_default();
            // The glyph names the *destination*, which is the rail's own rule
            // read back: the button turns the pane to whichever face it is not
            // showing.
            let to_source = !self.preview_md_source(surface);
            // **What the padlock says** — the very sentence the band used to
            // print, which is what makes the mark an honest replacement for it
            // rather than a mark nobody can read (owner's ruling 2026-09-12).
            let lock = self
                .preview_standing_fact(surface, Instant::now())
                .unwrap_or_default()
                .to_owned();
            for (tip, box_) in seats::preview_rail_tip_boxes(&geometry) {
                let text = preview_rail_tip_text(
                    tip,
                    measure.kind,
                    &segments,
                    &geometry.folded,
                    to_source,
                    &lock,
                );
                anchors.push(
                    tooltip::TooltipAnchorId::PreviewRail(surface, tip),
                    box_,
                    text,
                );
            }
        }
    }

    /// The tools **this seat's** head must be laid out with — this frame's, off
    /// the widths the paint stored for that seat.
    /// How many rows the switcher would put in front of you — **the list's own
    /// length, and not the pool's** (user ruling 2026-08-19).
    ///
    /// The chevron is the only way into that list, so gating it on the pool was
    /// gating a door on the size of a room it no longer opens onto: a kept file
    /// nobody has opened is a row of that list, and on the first frame after a
    /// restart it may be the only row there is. Found by a real window, where a
    /// pane showing one file could not be made to show the file the user had
    /// pinned in the session before.
    ///
    /// The badge beside the chevron keeps counting *buffers* rather than rows,
    /// because that is what it means — P19's honest inventory of hidden state —
    /// and a shortcut to a file nobody has opened is not hidden state.
    fn preview_switcher_rows(&self, seat: SeatId) -> usize {
        self.preview_menu_items(seat).len()
    }

    pub(crate) fn preview_head_tools(&self, seat: SeatId) -> seats::PreviewHeadTools {
        let surface = self.preview_here(seat);
        let buffer = self.preview_buffer_on(surface);
        let measured = self
            .window
            .preview_head_measures
            .get(&self.leaf_here(seat))
            .copied();
        seats::PreviewHeadTools {
            save: self.preview_is_editable(surface),
            // The dressing's own judgement, asked the same way: a markdown with
            // a breadcrumb row wears its flip down there, so the head does not
            // reserve a box for it (user ruling 2026-08-24).
            flip: buffer.is_some_and(|buffer| buffer.ftype == preview::PreviewFtype::Markdown)
                && self.preview_rail_kind(surface).is_none(),
            // **This pane is playing** (user ruling 2026-08-27; §7.23 ⑩), so
            // the per-type slot holds a stop instead of a flip. Asked of the
            // seat's mint through the one predicate every surface asks, so a
            // head that offers to stop something is a head over something that
            // is running.
            stop: self.surface_is_playing_a_video(surface),
            // **A page on this seat** (§7.7 ②). The seam slice ③ moves: today a
            // window has one page and it names its own seat, and when the
            // preview pool learns about pages this becomes a question about the
            // buffer above.
            web: self.seat_holds_a_page(seat),
            switcher: self.preview_switcher_rows(seat) > 1,
            // The one tool in this head that stands at rest, so the one the hit
            // test may answer for without a hover — see
            // [`seats::PreviewHeadTools::locked`].
            locked: self.seats.preview_is_locked(seat),
            // Zero until this seat's head has been drawn once, which is the
            // honest answer for a frame the paint has not reached yet: a pill
            // sized to no name is a pill nothing is inside, and the press it
            // misses is the press before the first frame.
            name_width: measured.map_or(0.0, |tools| tools.name_width),
            count_width: measured.map_or(0.0, |tools| tools.count_width),
        }
    }

    /// **What one control on a preview head says it does** (user ruling
    /// 2026-08-27).
    ///
    /// Exhaustive over [`seats::PreviewHeadTool`], which is the half of the
    /// ruling a table alone cannot carry: the registry says *which verbs stand
    /// bare on a head* and the gate holds it to having words for each
    /// (`icons::ActionIcon::bare_tip`), and this says which of a verb's words
    /// this frame's button is showing. Two of the six change with the state —
    /// the padlock says the action or the way out of it, the flip says which
    /// face it would turn to — and a single string for either would be the head
    /// describing what it was a press ago.
    ///
    /// Nothing here is allowed to answer with the empty string:
    /// `TooltipAnchors::push` would drop the anchor, and the control would be
    /// back where the report found it.
    pub(crate) fn preview_head_tool_tip(
        &self,
        seat: SeatId,
        tool: seats::PreviewHeadTool,
    ) -> String {
        let words = tool.verb().bare_tip().unwrap_or_default();
        let say = |at: usize| {
            words
                .get(at)
                .map_or_else(String::new, |text| text.text().to_owned())
        };
        match tool {
            // One word each: the save is the chord's own row (a button and a key
            // onto one room are one name), and the other two say one thing.
            seats::PreviewHeadTool::Save
            | seats::PreviewHeadTool::DevTools
            | seats::PreviewHeadTool::PopOut => say(0),
            // The rail's own two words for the same flip, from the same function
            // — a verb that survives its row keeps its wording too, and that
            // function knows about the third face a page has.
            seats::PreviewHeadTool::Flip => preview_rail_tip_text(
                seats::PreviewRailTip::Flip,
                seats::PreviewRailKind::Crumbs,
                &[],
                &[],
                !self.preview_md_source(self.preview_here(seat)),
                "",
            ),
            // **The second of that sign's two sentences.** The first is the
            // rail's stop-loading; this square is over a video.
            seats::PreviewHeadTool::Stop => say(1),
            // The action, or the way out of the state.
            seats::PreviewHeadTool::Lock => say(usize::from(self.seats.preview_is_locked(seat))),
        }
    }

    /// The path along the bottom of the preview pane, cut to the room it has —
    /// or the word it is flashing instead.
    ///
    /// **One strip, two confirmations, one duration** (P34, and ruling 6): the
    /// foot flashes "Saved" the same way it flashes "Revealed", and both stand
    /// for [`FOOT_REVEAL_FEEDBACK`].
    ///
    /// **And one standing phrase on its right hand** (user ruling, 2026-08-15).
    /// The strip's two halves answer two questions — "where is this file" on the
    /// left, "what is true of it" on the right — and a flash owns both: while
    /// the left is confirming, the right is empty, which is
    /// [`seats::dress_foot`]'s rule and not this function's.
    pub(crate) fn dress_preview_foot(
        &mut self,
        seat: SeatId,
        scale: f32,
        now: Instant,
    ) -> Option<seats::FootWords> {
        let surface = self.preview_here(seat);
        // **A page's foot is the hover line, and since 2026-08-25 nothing else —
        // and it is not a foot any more** (§7.7 ③ as twice amended). It was a
        // landing band as well until the page got an address row of its own; a
        // band that reprinted that row two lines below it was one address said
        // twice in one pane, which is the debt §7.7 booked and the first of the
        // two rulings settled. The second retired the band itself: what is left
        // is drawn as the bubble every browser puts in a page's bottom-left
        // corner (`seats::page_hover_tag_box`), so the words are dressed here
        // exactly as before and the *shape* they land in is the paint's.
        let page = self.web_on(seat).map(|web| web.page().clone());
        if let Some(page) = page {
            // **A page turned to its source is a document, and the bubble says a
            // document's phrase** (user ruling 2026-08-27; `docs/DESIGN.md`
            // §7.32). The far face edits and saves now, so it owes the same two
            // receipts every other editing surface in this window owes — the
            // flashed `Saved`, and the standing `Read-only · 64 KB` or
            // `Not saved · changed on disk` — and the one place a page has to
            // put a phrase is this corner.
            //
            // **One slot, one phrase, the fresher first**, which is
            // [`seats::dress_foot`]'s own rule read in a band that has only a
            // left hand: a confirmation owns the strip while it stands, and what
            // is underneath it comes back when it goes.
            //
            // **And the hover line is silent while the source is up.** The
            // pointer is over text this window drew; a link target left from the
            // last frame the browser was on the glass would be a sentence about
            // a page nobody is looking at.
            let sourced = self.page_source_shown_on(surface).is_some();
            let lead = if sourced {
                self.preview_save_notice(surface, now)
                    .or_else(|| self.preview_standing_fact(surface, now))
                    .unwrap_or_default()
                    .to_owned()
            } else {
                page_foot_lead(&page.url, &page.hover)
            };
            // **And the magnification, when the wheel has just moved it** (user
            // ruling 2026-08-25). The same word the picture beside it says, on
            // this band's own one flash clock — see [`page_foot_flash`], which
            // owns which of the two confirmations this strip is carrying.
            let opened = self
                .window
                .revealed_foot
                .filter(|(shown, _)| *shown == RevealedFoot::Preview(seat))
                .map(|(_, at)| at);
            let zoomed = self.web_on(seat).and_then(webhost::WebSeat::zoom_said);
            // The receipt, and the ninety milliseconds it takes to trade places
            // with the hover line — see [`Self::foot_saying`]. The two clocks in
            // front of it (`page_foot_flash`'s own) are untouched.
            // **Neither confirmation belongs to the source face.** A zoom is the
            // engine's magnification and an `Opened` is this window having handed
            // the file to a browser; both are about the page, and the page is not
            // what is on the glass. The phrase the source face owes is already in
            // `lead` above.
            let wanted = if sourced {
                None
            } else {
                page_foot_flash(
                    zoomed,
                    opened,
                    self.web_on(seat).and_then(webhost::WebSeat::dialog_said),
                    now,
                )
            };
            let (flash, dissolved) =
                self.foot_saying(FootSaying::PageTag(seat), wanted.as_deref(), now);
            let rect = seats::full_pane_rect(&self.seat_layout, seat)?;
            // **The tag's run and not the retired band's**: the cut has to be to
            // the room the thing that is drawn actually has, and a bubble
            // floating inside the body has a little less of it than a strip
            // spanning the pane did.
            let body = seats::preview_pane_geometry(rect, scale, self.seats.seat_rail(seat)).body;
            let margin = (seats::PAGE_HOVER_TAG_MARGIN_LOGICAL_PX * scale).round();
            let pad = (seats::PAGE_HOVER_TAG_PAD_X_LOGICAL_PX * scale).round();
            let run = [
                body[0] + margin + pad,
                body[1],
                (body[2] - margin - pad).max(body[0] + margin + pad),
                body[3],
            ];
            let font = seats::FILES_FOOT_FONT_LOGICAL_PX * scale;
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
            return Some(seats::dress_foot(
                seats::FootDress {
                    dissolved,
                    run,
                    lead: &lead,
                    // "Opened" is the same word the `Open in default app` card
                    // flashes, because it is the same sentence: this window
                    // handed the thing to the machine and the machine has it now.
                    flash: flash.as_deref(),
                    // A page has no standing fact to hang on the right: the two
                    // this strip carries — a truncated read and a refused save —
                    // are both about a *buffer*, and a page holds none.
                    notice: "",
                    // **A path is cut from the front and a URL from the back.**
                    // The scheme and the host are what identify a URL and the
                    // query is what is expendable, which is the opposite end
                    // from a path, whose file name is its identity. Same band,
                    // same ellipsis, opposite ends, because the two kinds of
                    // address are read from opposite ends.
                    cut_left: false,
                    font_px: font,
                    gap_px: seats::FILES_FOOT_NOTICE_GAP_LOGICAL_PX * scale,
                },
                &mut measure,
            ));
        }
        // **A path, because the left hand of this strip is "where is this".**
        // A document with no file behind it has no answer to that question in
        // this vocabulary, so it answers in its own: `folio · src/main.rs`, the
        // repository and the place in it, which is exactly the pair a path would
        // have carried (G-3, [`preview::PreviewSource::composed_lead`]).
        let lead = match self.preview_buffer_on(surface) {
            Some(buffer) => match buffer.source.file_path() {
                Some(path) => path.to_string_lossy().into_owned(),
                None => buffer.source.composed_lead().unwrap_or_default(),
            },
            None => self
                .preview_pane(surface)?
                .image
                .as_ref()?
                .path
                .to_string_lossy()
                .into_owned(),
        };
        // **The strip itself has retired where a breadcrumb took the question
        // over** (user ruling 2026-08-24) — `preview_pane_geometry` collapses it
        // and the paint draws nothing. What this function still owes such a seat
        // is the two phrases the ruling kept: the standing fact and the flashed
        // confirmation, which the frame lifts off this dressing and hangs on the
        // rail's right hand. So the words are still computed here, once, and the
        // *path* is what stops being printed: it is the one thing the row above
        // now says, and a band naming the file a row above it already names is
        // the duplication the ruling retired.
        let railed = self.preview_rail_kind(surface) == Some(seats::PreviewRailKind::Crumbs);
        let revealed = self.foot_reveal_is_fresh(RevealedFoot::Preview(seat), now);
        let saved = self.preview_save_notice(surface, now) == Some(preview::preview_saved_notice());
        let wanted = if revealed {
            Some(foot_revealed_label())
        } else if saved {
            Some(preview::preview_saved_notice())
        } else {
            None
        };
        let (flash, dissolved) = self.foot_saying(FootSaying::Preview(surface), wanted, now);
        let notice = self
            .preview_standing_fact(surface, now)
            .unwrap_or_default()
            .to_owned();
        let rect = seats::full_pane_rect(&self.seat_layout, seat)?;
        let run = seats::pane_foot_geometry(rect, scale).foot_path;
        let font = seats::FILES_FOOT_FONT_LOGICAL_PX * scale;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(seats::dress_foot(
            seats::FootDress {
                dissolved,
                run,
                lead: if railed { "" } else { &lead },
                flash: flash.as_deref(),
                notice: &notice,
                // **Left-truncated** (P35): the ellipsis goes at the head so the
                // file name — the part you actually care about — survives.
                cut_left: true,
                font_px: font,
                gap_px: seats::FILES_FOOT_NOTICE_GAP_LOGICAL_PX * scale,
            },
            &mut measure,
        ))
    }

    /// **Put a page's address on the clipboard** — the rail's `⧉` (user ruling
    /// 2026-08-24).
    ///
    /// The same clipboard door a file row's `Copy path` goes through, which is
    /// what makes the two rows' identical glyph an honest promise: one verb,
    /// two kinds of address.
    fn copy_preview_address(&mut self, surface: PreviewSurface) -> Result<()> {
        let Some(url) = self
            .rail_page(surface)
            .and_then(|leaf| self.window.web.get(&leaf))
            .map(|web| web.page().url.clone())
        else {
            return Ok(());
        };
        if url.is_empty() {
            return Ok(());
        }
        let result = write_terminal_clipboard_text(&url);
        recoverable_clipboard_write(result, "copy a page's address");
        Ok(())
    }

    /// **Stand the files column where this segment of the path is** (user ruling
    /// 2026-08-24) — a press on one breadcrumb.
    ///
    /// [`Self::locate_folder_in_files_column`] and nothing else, which is the
    /// whole of the ruling's 「查 files 列的既有跳转动词复用」: that verb already
    /// opens the way down inside the tree a tab has, re-roots when the folder is
    /// outside it, opens a column for a tab that has none, and leaves the
    /// keyboard where it was. A second implementation here would be a second
    /// answer to "what does pointing the column somewhere mean".
    ///
    /// **It was the hard verb until 2026-08-25**, and the ruling that softened
    /// it is why: a breadcrumb segment is by construction an ancestor of a file
    /// this window is already showing, so re-rooting on it threw away a tree the
    /// reader had built up in order to show them a subtree of it.
    ///
    /// **The last segment is this file, and a column is rooted at a directory**,
    /// so the tail hands over its parent. That is not a special case bolted on:
    /// every segment hands over the place it *names*, and the place a file names
    /// is the folder it is in.
    /// Whether this depth is the crumb rail's **last** segment — the file
    /// itself, and the only one a second press opens a box on (B5).
    ///
    /// Asked of the path rather than of the drawn geometry, because the drawn
    /// run is folded: `PreviewCrumbBox::depth` is an index into the whole path
    /// and the tail is its last component whether or not the middle survived the
    /// fold.
    fn preview_crumb_is_the_tail(&self, surface: PreviewSurface, depth: usize) -> bool {
        self.preview_rail_path(surface)
            .is_some_and(|path| depth + 1 == crumb_segments(&path).len())
    }

    fn press_preview_crumb(&mut self, surface: PreviewSurface, depth: usize) -> Result<()> {
        let Some(path) = self.preview_rail_path(surface) else {
            return Ok(());
        };
        let segments = crumb_segments(&path);
        let Some((_, target)) = segments.get(depth) else {
            return Ok(());
        };
        let folder = if depth + 1 == segments.len() {
            target.parent().map(Path::to_path_buf)
        } else {
            Some(target.clone())
        };
        let Some(folder) = folder else {
            return Ok(());
        };
        self.locate_folder_in_files_column(&folder)
    }

    /// **One control of a preview rail, pressed — the ladder both hosts run**
    /// (§7.7 ⑩ 欠账, 2026-08-25).
    ///
    /// The row grew inside a pane and was pressed through [`seats::ChromeTarget`],
    /// which names a seat; a torn-off window's is pressed through
    /// [`float::FloatPart::Rail`], which names no seat and never will. Writing
    /// the second host's answers beside the first's would have been eleven verbs
    /// spelled twice, and the pair that matters most — the address field and the
    /// breadcrumb's rename box — are the two this window has already had to
    /// repair for saying one thing in two places. So the *parts* are what a
    /// press is resolved to on both hosts ([`seats::preview_rail_part`]), and
    /// this is where a part becomes a verb.
    ///
    /// Every branch breaks a click chain for `.files-foot`'s reason — a chain of
    /// clicks on a button is a chain of button presses — and none of them arms a
    /// drag: this row is not a drag handle on either host, so there is nothing to
    /// decline.
    pub(crate) fn press_preview_rail(
        &mut self,
        surface: PreviewSurface,
        part: seats::PreviewRailPart,
    ) -> Result<()> {
        self.window.tab_clicks.interrupt();
        self.window.files_row_clicks.interrupt();
        // Which of the two fillings this row is wearing, asked once: two of the
        // controls below are one glyph with a sentence for each row, and asking
        // twice is two chances to disagree about one band.
        let kind = self
            .preview_rail_measure(surface)
            .map(|measure| measure.kind);
        match part {
            seats::PreviewRailPart::Back => self.run_web_head_verb(surface, WebHeadVerb::Back),
            seats::PreviewRailPart::Forward => {
                self.run_web_head_verb(surface, WebHeadVerb::Forward)
            }
            seats::PreviewRailPart::Reload => self.run_web_head_verb(surface, WebHeadVerb::Reload),
            seats::PreviewRailPart::Browser => self.open_preview_in_browser(surface),
            seats::PreviewRailPart::Flip => self.flip_preview_source_on(surface),
            seats::PreviewRailPart::OpenWith => self.open_preview_rail_menu(surface),
            // One glyph, two verbs, and the row's kind is what tells them apart
            // — the same fork [`seats::preview_rail_target`] makes on the docked
            // host, made once more where the press turns into an action.
            seats::PreviewRailPart::Copy => match kind {
                Some(seats::PreviewRailKind::Address) => self.copy_preview_address(surface),
                Some(seats::PreviewRailKind::Crumbs) => match self.preview_rail_path(surface) {
                    Some(path) => self.copy_path_to_clipboard(&path),
                    None => Ok(()),
                },
                None => Ok(()),
            },
            // **A press on the address is a caret in it**, and the field it
            // opens is the very one `Ctrl+L` opens: `open_web_address_on` is the
            // one door, so a URL typed after a click and one typed after the
            // chord cannot be seeded differently — on either host.
            seats::PreviewRailPart::Address => {
                self.window.preview_name_clicks.interrupt();
                match self.rail_page(surface) {
                    Some(leaf) => self.open_web_address_on(leaf),
                    None => Ok(()),
                }
            }
            // **A single press locates, a double press on the *last* segment
            // renames** (B5, user ruling 2026-08-25).
            //
            // The chain is keyed by the *surface* and not by the depth, and the
            // tail is the one segment it is armed on: every other segment is a
            // folder this pane is not showing, and there is nothing about a
            // folder for a box on this row to change. Two presses on a folder
            // are two locates — which is what they already were, and it costs
            // the tail nothing to say so, because a folder never enters the
            // chain at all.
            //
            // The single press still runs on the way past. That is the head's
            // own arrangement (`PreviewName`): the first half of a double click
            // is a real click, and locating the folder a file lives in is a move
            // the box being opened over it does not undo.
            seats::PreviewRailPart::Crumb(depth) => {
                let tail = self.preview_crumb_is_the_tail(surface, depth);
                if !tail {
                    self.window.preview_crumb_clicks.interrupt();
                }
                self.press_preview_crumb(surface, depth)?;
                if tail
                    && self
                        .window
                        .preview_crumb_clicks
                        .register(surface, Instant::now())
                        == TabClick::Double
                {
                    self.open_preview_crumb_rename(surface)?;
                }
                Ok(())
            }
            seats::PreviewRailPart::Fold => self.open_preview_crumb_menu(surface),
            // The band itself, everywhere its controls are not: it swallows the
            // press and does nothing with it. See [`seats::preview_rail_part`] —
            // a row that let a press through would either drag the pane by
            // something that is not its head or put a caret in the document
            // below.
            seats::PreviewRailPart::Band => Ok(()),
        }
    }

    /// **The `…` a folded path is drawn as** (the ruling's ③, re-judged by the
    /// user on 2026-08-25): the list of the levels standing behind it.
    ///
    /// **What it was, and why that was wrong.** It used to raise the ordinary
    /// file menu — `Open in default app`, `Show in files column`, `Copy path`,
    /// `Insert path` — on the *deepest* folder the fold was hiding, on the
    /// argument that the reader gets the whole path in it because `Copy path` is
    /// one of its rows. Two things are wrong with that and the ruling names
    /// both: the verbs are about **one** of several hidden folders and nothing
    /// on screen says which, and a control whose whole meaning is *there are
    /// folders here you cannot see* was answered by a menu that never says what
    /// they are.
    ///
    /// **What it is.** Windows Explorer's own breadcrumb `…`, which is the
    /// reference the ruling handed over: the hidden levels, one per row, each
    /// wearing a folder; **deepest first**, so the nearest hidden level is the
    /// top row and the list walks towards the root. A press on a row is
    /// [`Self::locate_folder_in_files_column`] — the same verb a press on a
    /// *visible* segment is, which is the whole of 「语义与点可见段一致」.
    ///
    /// The file verbs are not lost; they belong to `Open ⌄`, which is the
    /// control that is about this document.
    fn open_preview_crumb_menu(&mut self, surface: PreviewSurface) -> Result<()> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(geometry) = self.rail_geometry(surface, scale) else {
            return Ok(());
        };
        let Some(chip) = geometry.fold else {
            return Ok(());
        };
        let Some(path) = self.preview_rail_path(surface) else {
            return Ok(());
        };
        let crumbs = folded_levels(&path, &geometry.folded);
        // The deepest hidden level is what a press with no row on it would have
        // been about, and it is what the menu is hung *from*: an empty list is
        // an unfolded rail, which this door is not reached from.
        let Some(deepest) = crumbs.first().map(|level| level.folder.clone()) else {
            return Ok(());
        };
        // Under the chip, where a menu belonging to a control belongs when no
        // pointer said otherwise — the placement the root menu takes under its
        // own button.
        self.open_file_menu(
            FileMenuTarget {
                row: None,
                // Every row of this face carries its own destination, so the
                // target's own path is only what [`Self::open_file_menu`]'s
                // "this menu is about something" gate reads.
                activation: RowActivation::DefaultApp(deepest),
                subject: profiles::FileMenuSubject::FoldedPath {
                    levels: crumbs.len(),
                },
                crumbs,
                // **The chip is a trigger of its own** (owner's report
                // 2026-09-13): it raises this menu, so a press on it while its
                // menu is up is that menu's close and goes no further. It is the
                // pill's neighbour and not the pill — its list is the folded
                // path's, and the hover rule the pill is under is written
                // against the pill's own box, which is what
                // [`FileMenuState::rail`] answers `None` for here.
                trigger: Some(PopoverTrigger::Rail(surface, seats::PreviewRailPart::Fold)),
            },
            [chip[0], chip[3]],
        )
    }

    /// **`Open ⌄`** (user ruling 2026-08-24) — the breadcrumb row's one pill.
    ///
    /// The file menu again, raised on the document this seat is showing and hung
    /// under the pill. **The same menu machine a right press on a file row
    /// raises, and its own list of rows** — which is the composition the two
    /// rulings landed on: 「系统默认程序 / 在 files 列中定位」 are verbs about a
    /// path, so a window that grew a second popup for them here would be a
    /// window where the two could come to disagree; but a surface that *is* the
    /// preview must not offer `Open preview`, and a menu hung a band under the
    /// pane's own `Reveal in Explorer` must not offer that either.
    /// [`profiles::FileMenuSubject::Document`] is where the difference is
    /// written down.
    ///
    /// **The file verbs live here and only here** since 2026-08-25: the `…`
    /// chip beside this pill used to raise the same list on a folder nobody had
    /// named, and it lists its own hidden levels now
    /// ([`Self::open_preview_crumb_menu`]). One control about this document, one
    /// control about the path behind the fold.
    pub(crate) fn open_preview_rail_menu(&mut self, surface: PreviewSurface) -> Result<()> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(pill) = self
            .rail_geometry(surface, scale)
            .and_then(|rail| rail.open)
        else {
            return Ok(());
        };
        let Some(path) = self.preview_rail_path(surface) else {
            return Ok(());
        };
        self.open_file_menu(
            FileMenuTarget {
                row: None,
                activation: RowActivation::DefaultApp(path),
                subject: profiles::FileMenuSubject::Document,
                crumbs: Vec::new(),
                // **Whose menu this is** — for the hover clock, which is about
                // this one control, and for the press router, which spends a
                // second press on the pill closing what the first one opened.
                trigger: Some(PopoverTrigger::Rail(
                    surface,
                    seats::PreviewRailPart::OpenWith,
                )),
            },
            [pill[0], pill[3]],
        )
    }

    /// Show the file on the seat in File Explorer — `.preview-pane .files-foot`
    /// (P32), the same verb and the same confirmation a files column's foot has.
    pub(crate) fn reveal_preview_file(&mut self, seat: SeatId) -> Result<()> {
        // **A page's band hands the page over instead** (§7.7 ③): 「外链一律外
        // 交」, and this is the one place a web seat has to say it. The flash is
        // the same flash — one band, one confirmation, one duration.
        if let Some(url) = self.web_on(seat).map(|web| web.page().url.clone()) {
            self.hand_url_to_the_browser(&url)?;
            self.window.revealed_foot = Some((RevealedFoot::Preview(seat), Instant::now()));
            self.refresh_chrome();
            return self.present_chrome_change();
        }
        let surface = self.preview_here(seat);
        // **The one file verb a composed document keeps** (G-3). Explorer points
        // at files, and a diff has none — but the diff is *of* a file, and that
        // file is in the working tree where Explorer can point at it. Offered
        // only when it is still there: a diff of a deletion names a file that is
        // gone, and asking Explorer to highlight it would open its folder with
        // nothing selected, which is a verb quietly doing something else.
        let Some(path) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| revealable_preview_file(&buffer.source))
            .or_else(|| {
                self.preview_pane(surface)
                    .and_then(|pane| pane.image.as_ref())
                    .map(|image| image.path.clone())
            })
        else {
            return Ok(());
        };
        // **The file itself, highlighted** (user ruling, 2026-08-13). This used
        // to hand Explorer the *parent folder*, because the only door out was
        // `open_local_path` and asking that to open a text file would have run
        // it. `reveal_in_explorer` is the door that asks Explorer to point at
        // something rather than to open it, so the foot can finally name what
        // it prints: in a folder of two hundred rows, "the directory it is in"
        // is not an answer to "where is this file".
        if !self.reveal_in_explorer(&path) {
            return Ok(());
        }
        self.window.revealed_foot = Some((RevealedFoot::Preview(seat), Instant::now()));
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Hand the page on this seat to the machine's own browser — the head's `↗`
    /// (user ruling 2026-08-20, §7.1.5g).
    ///
    /// **The same one door out this window has always had**,
    /// [`Self::open_local_path`], and through it `bt_platform`'s own refusal for
    /// the executable list — a refusal this route cannot reach (nothing on that
    /// list is named `.html`) and is subject to anyway, because the rule belongs
    /// to the door rather than to the call sites that knock on it.
    ///
    /// **This seat's own buffer and not the focused one**: the button stands on a
    /// head, and a head belongs to a pane. Asking `seats.preview()` for it here
    /// would hand over whatever the *focused* seat happened to be showing, which
    /// on a window with two preview panes is the other one.
    ///
    /// **No confirmation is printed.** The card's `Opened ✓` exists because the
    /// card is still on screen with nothing else to say, and the foot's
    /// `Revealed` because Explorer can open behind you; a browser handed a page
    /// comes to the front, and a window that has just lost the foreground has no
    /// reader to print a word for.
    fn open_preview_in_browser(&mut self, surface: PreviewSurface) -> Result<()> {
        let Some(path) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| preview_page_hand_off(&buffer.source))
        else {
            return Ok(());
        };
        self.open_local_path(&path);
        Ok(())
    }

    /// Turn a preview over — the render ⇄ the source (P28).
    ///
    /// **The flip is a property of the view, not of the buffer** (ruling
    /// 2026-08-13, overturning the reading this carried until then). It followed
    /// the dirty bit's rule — one buffer per file, so which face it wore
    /// travelled with it — and the machine showed what that cost: the pool is
    /// the tab's, so turning one surface over turned every surface on that file
    /// over, and a float torn off to read a page became raw markdown because a
    /// pane behind it had been flipped. Reading the render in one place while
    /// editing the source in another is a legitimate thing to want, and it is
    /// ruling 8⑧'s line drawn one field further along: the file is shared, the
    /// way you are looking at it is not.
    ///
    /// **A surface, and never "whichever one has the keyboard"** (real-machine
    /// defect, 2026-08-26). There was a second door here that took no argument
    /// and asked `preview_keyboard_surface`; it survived because pressing a
    /// markdown pane's head is what gives that pane the keyboard, so the two
    /// answers were usually the same one. A page's address row ended that — see
    /// the `ChromeTarget::PreviewFlip` arm — and the door with no argument is
    /// gone rather than left standing for the next caller to reach for.
    pub(crate) fn flip_preview_source_on(&mut self, surface: PreviewSurface) -> Result<()> {
        // **A page's two faces are turned by the same button** (user ruling
        // 2026-08-26; DESIGN §7.7 ⑭), and the arm is chosen by the seat rather
        // than by the buffer: a page seat's buffer is its `PreviewSource::Web`,
        // which has no ftype worth asking, and the offer is a property of the
        // address the engine is standing on.
        if let Some(path) = self.preview_page_source_file(surface) {
            return self.flip_page_source_on(surface, path);
        }
        // The *file* decides whether there is a face to turn — only markdown has
        // two — and the *surface* is what gets turned.
        if self
            .preview_buffer_on(surface)
            .is_none_or(|buffer| buffer.ftype != preview::PreviewFtype::Markdown)
        {
            return Ok(());
        }
        let pane = self.preview_pane_mut(surface);
        pane.md_source = !pane.md_source;
        // **Turning to the source face is asking to edit** (T2 ③, 2026-09-10) —
        // the far side of this flip is the one with a caret in it, so if the
        // glance only bought the head of this document, this is where the rest
        // of it is bought. Asked after the flag has moved, because the buffer is
        // judged as the surface is now showing it; and asked on the way back as
        // well, where it costs nothing — a buffer that already has the whole
        // file, or has been told the file is too large, refuses at the door.
        self.ask_to_edit_preview_on(surface);
        // The keyboard cannot stay in an editor that is no longer on screen. The
        // flag is healed at the read (`preview_edit_focus`), so this is belt and
        // braces — but the caret it would otherwise leave behind is not, and a
        // caret parked in a rendered document is a caret in nothing.
        if self.preview_edit_focus == Some(surface) {
            self.preview_edit_focus = None;
        }
        self.repaint_preview()
    }

    /// Lock or unlock the preview pane (P30/P95).
    ///
    /// **The lock is the only route to a second preview pane** — "this pane
    /// keeps its buffers and stops being the reuse target; what opens NEXT opens
    /// in a fresh preview beside it" — and it is deliberately the only one:
    /// editing does not promote (`DESIGN.md` §7.1.3, user ruling 2026-07-17,
    /// which overturned the same day's earlier "编辑即转正"). The state is
    /// durable, because it is a fact about the pane and not about this session's
    /// pointer.
    ///
    /// **It was called a pin until 2026-08-23** (§7.7 ⑧): this window's two real
    /// pins both mean "this one stays in the list", and holding a pane against
    /// reuse is not that. The durable flag under it keeps the old word, which is
    /// [`seats::Seats::preview_is_locked`]'s own note.
    pub(crate) fn toggle_preview_lock(&mut self, seat: SeatId) -> Result<()> {
        if !self.seats.toggle_preview_lock(seat) {
            return Ok(());
        }
        self.mark_session_dirty(Instant::now());
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// A wheel notch over a preview's text body.
    ///
    /// [`Self::scroll_files_tree`]'s twin, clamped at both ends by the same
    /// authority and for the same reason (R2 乙案): a wheel at the end of a
    /// document that goes on adding to a number nothing reads is a body that
    /// costs one notch per notch to come back up.
    ///
    /// The text surface is rebuilt rather than repainted, because the lines
    /// *are* the offset: which of them exist this frame is what scrolling
    /// changes.
    pub(crate) fn scroll_preview_body(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
        delta: MouseScrollDelta,
    ) -> Result<()> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // **A report the platform already put on the x axis is sideways, and it
        // needs no modifier to say so** (user ruling, 2026-09-07). A tilt wheel
        // and a touchpad's second finger have been arriving here since this
        // method was written and being spent on `y`, which for a report that is
        // all `x` is zero: the notch reached the right surface and did nothing at
        // all. `wheel_points_sideways` is the same reading the terminal's own
        // axis takes, and `wheel_travel` then takes the component that goes with
        // it.
        //
        // **Shift turns the wheel sideways** as well, which is the convention
        // every horizontal scroller on this desktop follows and the only one a
        // mouse without a second axis can reach. Nothing about it changes here or
        // in the terminal: over a preview it already meant this axis, and what
        // 2026-09-07 adds is a road to the same place that needs no hand on the
        // keyboard. The notch is the same notch either way.
        let tilted = wheel_points_sideways(delta);
        let sideways = tilted || self.window.modifiers.shift_key();
        let extent = if sideways {
            body[2] - body[0]
        } else {
            body[3] - body[1]
        };
        let travel = self.wheel_travel(delta, extent, tilted);
        // **Sideways over a wide block scrolls the block** (user ruling,
        // 2026-08-13). Asked first, because the page has no horizontal axis of
        // its own on this surface and a notch spent on nothing is a notch the
        // user has to spend again.
        if sideways && self.scroll_preview_block(surface, body, scale, travel)? {
            return Ok(());
        }
        let scroll = self.preview_pane_mut(surface).scroll;
        let mut wanted = scroll;
        wanted[usize::from(!sideways)] -= travel;
        let scrolled = self.clamped_preview_scroll(surface, body, scale, wanted);
        if scrolled == scroll {
            return Ok(());
        }
        self.preview_pane_mut(surface).scroll = scrolled;
        self.refresh_preview_body();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// A sideways notch spent on the wide block under the pointer, if there is
    /// one.
    ///
    /// Returns whether a block took it. **The pointer decides which block**, not
    /// the focus and not the caret: a page may hold several wide tables, they are
    /// separate scrolling regions, and the one you are pointing at is the one you
    /// mean — which is what every browser does with a `overflow-x: auto` div and
    /// the only rule that needs no second control to disambiguate.
    fn scroll_preview_block(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
        travel: f32,
    ) -> Result<bool> {
        let Some(pane) = self.preview_pane(surface) else {
            return Ok(false);
        };
        let PreviewDocument::Markdown { blocks, layout, .. } = &pane.doc else {
            return Ok(false);
        };
        let Some(pointer) = self.window.pointer_position else {
            return Ok(false);
        };
        let metrics = seats::preview_markdown_metrics(scale);
        let Some((index, offset)) = preview_block_wheel(
            body,
            metrics,
            pane.scroll,
            &pane.md_block_scroll,
            (blocks, layout),
            [pointer.x as f32, pointer.y as f32],
            travel,
        ) else {
            return Ok(false);
        };
        self.set_preview_block_scroll(surface, index, offset)?;
        Ok(true)
    }

    /// Where a block's own offset is written — the wheel's answer and the
    /// thumb's, through one door.
    ///
    /// Reports whether anything moved. The caller does the clamping, because the
    /// two gestures clamp against the same numbers by different arithmetic and
    /// this is not the place to guess which one is asking.
    fn set_preview_block_scroll(
        &mut self,
        surface: PreviewSurface,
        index: usize,
        offset: f32,
    ) -> Result<bool> {
        let offsets = &mut self.preview_pane_mut(surface).md_block_scroll;
        // Grown here rather than at the rebuild, because a document is parsed far
        // more often than a block is scrolled and a vector of zeros per parse is
        // a cost paid for a case that usually does not arise.
        if offsets.len() <= index {
            offsets.resize(index + 1, 0.0);
        }
        if offsets[index] == offset {
            return Ok(false);
        }
        offsets[index] = offset;
        self.repaint_preview()?;
        Ok(true)
    }

    /// The box **this surface's document** is drawn in, the glance card
    /// included.
    ///
    /// [`Self::preview_surface_body_rect`] answers for everything that has a
    /// place in the tree or in the float host and refuses the card, which is in
    /// neither; the card's box is the one its own layout produced and filed on
    /// [`FilePeek::body`]. A gesture that took hold of a block inside the card
    /// has to be able to ask *after the fact* where that document is, and this
    /// is the reading that can answer for all three surfaces.
    fn preview_document_box(&self, surface: PreviewSurface, scale: f32) -> Option<[f32; 4]> {
        if surface == PreviewSurface::Peek {
            let peek = self.window.file_peek.as_ref()?;
            return peek.body.filter(|_| peek.clock.is_shown());
        }
        self.preview_surface_body_rect(surface, scale)
    }

    /// **Which surface a question about a wide block belongs to** — the pane
    /// walk, with the glance card in front of it (user ruling, 2026-08-14).
    ///
    /// A table too wide for the card is the same scrolling region a table too
    /// wide for the pane is, and the ruling that gave the card a wheel and a
    /// thumb gave it the blocks inside it too: what a hand can do to a document
    /// in a pane it can do to the same document in the glance of it. The wheel
    /// already agreed — [`Self::mouse_wheel`] hands a notch over the card to
    /// [`Self::scroll_preview_body`] with [`PreviewSurface::Peek`] — and this is
    /// the half that was missing, the half a *hand on the bar* goes through.
    ///
    /// The card is asked first because it is the topmost thing on the glass, and
    /// it has to be asked *here* rather than inside
    /// [`Self::preview_surface_at`]: that walk answers presses, carets,
    /// selections and the edit focus as well, and the card is read-only —
    /// putting it in there would hand it every one of those. So the card enters
    /// exactly the questions it is entitled to, through
    /// [`file_peek::body_at`].
    fn preview_block_surface_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(PreviewSurface, [f32; 4])> {
        let at = [position.x as f32, position.y as f32];
        if let Some(peek) = self
            .window
            .file_peek
            .as_ref()
            .filter(|peek| peek.clock.is_shown())
            && let Some((frame, body)) = peek.frame.zip(peek.body)
            && let Some(body) = file_peek::body_at(frame, body, at)
        {
            return Some((PreviewSurface::Peek, body));
        }
        self.preview_surface_at(position)
    }

    /// The bar the pointer is on, asked of the frame as it stands — and of
    /// whichever surface the pointer is in.
    fn preview_block_bar_under(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(PreviewSurface, usize, preview::ScrollBar)> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (surface, body) = self.preview_block_surface_at(position)?;
        let pane = self.preview_pane(surface)?;
        let PreviewDocument::Markdown { blocks, layout, .. } = &pane.doc else {
            return None;
        };
        let (index, bar) = preview_block_bar_at(
            body,
            seats::preview_markdown_metrics(scale),
            pane.scroll,
            &pane.md_block_scroll,
            (blocks, layout),
            scale,
            [position.x as f32, position.y as f32],
        )?;
        Some((surface, index, bar))
    }

    /// Which of this surface's thumbs is lit this frame, and whether it is in a
    /// hand.
    ///
    /// Asked per surface because the answer is drawn per surface: there is one
    /// pointer, so at most one pane's thumb is ever lit — and the panes it is not
    /// in have to be told "none of yours" rather than be handed the other's
    /// index.
    fn preview_block_lit(&self, surface: PreviewSurface) -> Option<(usize, bool)> {
        self.preview_block_drag
            .filter(|drag| drag.surface == surface)
            .map(|drag| (drag.index, true))
            .or(self
                .preview_block_hover
                .filter(|(hovered, _)| *hovered == surface)
                .map(|(_, index)| (index, false)))
    }

    /// **This surface's vertical bar**, as it stands right now.
    ///
    /// Re-derived from the live geometry on every question rather than stored,
    /// which is the block bar's own hard-won rule and the card's: a rectangle
    /// remembered from the frame it was drawn on is a rectangle the pointer is
    /// tested against after the thing moved.
    ///
    /// The `body` is handed in because one caller cannot ask for it — the glance
    /// card's box is a number its own layout produced and it is in no tree; see
    /// [`Self::preview_surface_pane_rect`]'s third arm.
    pub(crate) fn preview_surface_bar(
        &self,
        surface: PreviewSurface,
        axis: preview::ScrollAxis,
        body: [f32; 4],
        scale: f32,
    ) -> Option<preview::ScrollBar> {
        let scroll = self.preview_pane(surface)?.scroll;
        let content = match axis {
            // **The width the offset is already clamped against**, read back out
            // of the clamp rather than measured a second time: `max_scroll` is
            // `content - page` by construction in every one of the four
            // geometries, so `page + max_scroll` is the content, exactly, for
            // all of them — and a bar built from a second measurement is a thumb
            // that stops where the wheel does not.
            preview::ScrollAxis::Horizontal => {
                let page = body[2] - body[0];
                page + self.preview_max_scroll(surface, body, scale)[0]
            }
            preview::ScrollAxis::Vertical => {
                self.preview_surface_document_height(surface, body, scale)
            }
        };
        preview_body_bar(body, axis, scroll, content, scale)
    }

    /// Both of a surface's bars, in the order their offsets sit in `scroll`.
    ///
    /// A surface wears none, one or two: a wrapped markdown page has no
    /// horizontal axis at all, a short wide patch has only the horizontal, and
    /// an unwrapped source file longer and wider than its pane has both.
    fn preview_surface_bars(
        &self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
    ) -> [Option<preview::ScrollBar>; 2] {
        // **A float's rule is cut from the inset box and the document is not**
        // (§7.39's report of 2026-08-28, kept through the ruling of
        // 2026-09-12). The body reaches the window's floor now, where the two
        // corners curve and the grip sits; text drawn there is clipped by the
        // face around it and reads correctly, and a scrollbar drawn there is a
        // straight rule laid out over a curve.
        let track = match surface {
            PreviewSurface::Float(id) => self.float_inset_body_rect(id, scale).unwrap_or(body),
            PreviewSurface::Seat(_) | PreviewSurface::Peek => body,
        };
        [
            self.preview_surface_bar(surface, preview::ScrollAxis::Horizontal, track, scale),
            self.preview_surface_bar(surface, preview::ScrollAxis::Vertical, track, scale),
        ]
    }

    /// That bar, painted — or nothing when the whole document fits.
    ///
    /// A layer of its own, above whatever it is riding over: see
    /// [`scroll_bar_layer`] for why a bar cannot be more quads on the surface it
    /// belongs to.
    fn preview_body_bar_layers(
        &self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
    ) -> Vec<marks::OverlayLayer> {
        let palette = bt_render::chrome_palette();
        self.preview_surface_bars(surface, body, scale)
            .into_iter()
            .flatten()
            .map(|bar| {
                let state = ScrollThumbState::of(
                    self.preview_body_drag
                        .is_some_and(|drag| drag.surface == surface && drag.axis == bar.axis),
                    self.preview_body_hover == Some((surface, bar.axis)),
                );
                scroll_bar_layer(&bar, state, &palette)
            })
            .collect()
    }

    /// Every **docked** preview pane's bar, in seat order.
    ///
    /// The floats' are not here: a window's bar has to be drawn directly above
    /// that window and below the next one, so it rides in the float family
    /// beside the window it belongs to ([`Self::float_layer`]). These are the
    /// panes in the tree, and they belong at the very bottom of the overlay —
    /// see [`OverlayStack::preview_bars`].
    pub(crate) fn preview_seat_bar_layers(&self) -> Vec<marks::OverlayLayer> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        self.seats
            .preview_seats()
            .into_iter()
            .flat_map(|seat| {
                let surface = self.preview_here(seat);
                let Some(body) = self.preview_surface_body_rect(surface, scale) else {
                    return Vec::new();
                };
                self.preview_body_bar_layers(surface, body, scale)
            })
            .collect()
    }

    /// One preview float's bars, on their own layers above that window.
    /// **Every surface a pointer could be asking a video about, front to back**
    /// (route B slice ②, 2026-08-28; §7.44 ②).
    ///
    /// The order is the z-order and nothing else: the glance card stands over
    /// every float, a float stands over every pane, and the topmost thing under
    /// the pointer is the one the gesture belongs to. Written once here rather
    /// than repeated at the press, the drag and the hover, because three
    /// orders is how two of them come to disagree.
    fn video_pointer_surfaces(&self) -> Vec<PreviewSurface> {
        let mut surfaces = vec![PreviewSurface::Peek];
        surfaces.extend(
            self.window
                .float
                .drawn()
                .map(|win| PreviewSurface::Float(win.epoch))
                .collect::<Vec<_>>()
                .into_iter()
                .rev(),
        );
        surfaces.extend(
            self.window
                .video
                .iter()
                .map(|(surface, _)| surface)
                .filter(|surface| matches!(surface, PreviewSurface::Seat(_))),
        );
        surfaces
    }

    /// **The bar of one surface, laid out** — `None` when nothing is playing
    /// there or the bar is not up.
    ///
    /// The same [`video_seat::bar_layout`] the painter used, off the same
    /// [`Self::video_shape_of`] rectangle: a hit test that derived the boxes a
    /// second way would be a bar that answers a hand somewhere other than where
    /// it was drawn, which is the one defect a control bar cannot have.
    fn video_bar_layout_of(&self, surface: PreviewSurface) -> Option<video_seat::BarLayout> {
        let seat = self.window.video.get(surface)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let now = Instant::now();
        if seat.presence(now, self.app.motion).opacity <= 0.0 {
            return None;
        }
        let shape = self.video_shape_of(surface, scale, now)?;
        let state = seat.state();
        let figures = video_seat::clock_figures(state.position_secs, state.duration_secs);
        video_seat::bar_layout(
            shape.rect(),
            scale,
            figures,
            self.video_meta_of(surface).width,
        )
    }

    /// **The play button over a surface showing a recording it is not playing.**
    ///
    /// The same disc [`seats::preview_play_button_box`] cuts for a pane, asked
    /// of all three surfaces — which is the ruling's *「能动的就动」* reaching a
    /// glance card: a card with a first frame on it and no way to start it is a
    /// card that knows the file is a video and will not say so.
    fn video_play_button_of(&self, surface: PreviewSurface) -> Option<[f32; 4]> {
        if self.window.video.get(surface).is_some() {
            return None;
        }
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (body, path) = match surface {
            // A docked pane's button is cut by the chrome pass, hit-tested by
            // `seats::hit_preview_play`, and is not this function's to answer a
            // second time — see [`Self::seats_wearing_a_play_button`].
            PreviewSurface::Seat(_) => return None,
            PreviewSurface::Float(_) => (
                self.preview_surface_body_rect(surface, scale)?,
                self.preview_picture(surface)
                    .map(|image| image.path.clone())?,
            ),
            PreviewSurface::Peek => {
                let body = self.window.file_peek.as_ref()?.body?;
                let path = self.file_peek_subject()?.path?;
                (file_peek::page_ground(body, scale), path)
            }
        };
        if !preview::path_names_a_video(&path) {
            return None;
        }
        seats::preview_play_button_box(body, scale)
    }

    /// The play button of a float or a card, as a layer — the pane's is cut by
    /// the chrome pass instead, which is the one place it has always been.
    pub(crate) fn video_play_mark_layer(
        &self,
        surface: PreviewSurface,
    ) -> Option<marks::OverlayLayer> {
        let button = self.video_play_button_of(surface)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let hovered = self
            .window
            .pointer_position
            .is_some_and(|at| file_peek::contains(button, [at.x as f32, at.y as f32]));
        // The **same** box the button was cut from, which is the box
        // `video_play_button_of` measured against — a mark drawn from a second
        // derivation would be a disc a press could miss.
        let body = match surface {
            // A recording's card, and therefore always `page_ground` — an
            // animation never wears a play mark.
            PreviewSurface::Peek => {
                file_peek::page_ground(self.window.file_peek.as_ref()?.body?, scale)
            }
            _ => self.preview_surface_body_rect(surface, scale)?,
        };
        Some(marks::OverlayLayer {
            sprites: seats::preview_play_button_sprites(
                body,
                hovered,
                scale,
                &bt_render::chrome_palette(),
            ),
            ..marks::OverlayLayer::default()
        })
    }

    /// **A press somewhere a video is involved** — `true` when it was taken.
    ///
    /// Asked ahead of the chrome ladder for [`Self::chrome_target_at`]'s own
    /// reason said one level up: a control standing on a picture out-ranks every
    /// gesture on the picture, and a bar standing on a *float* or a *card* is
    /// not in that ladder at all, because neither surface is this tab's chrome.
    ///
    /// **And ahead of `hide_file_peek`**, which is the one ordering that is not
    /// obvious: P149 takes the glance card down on every press before the press
    /// means anything, and a press on the card's own play mark would otherwise
    /// dismiss the card it was starting.
    pub(crate) fn press_video_at(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let at = [position.x as f32, position.y as f32];
        let now = Instant::now();
        for surface in self.video_pointer_surfaces() {
            if let Some(button) = self.video_play_button_of(surface)
                && file_peek::contains(button, at)
            {
                let path = match surface {
                    PreviewSurface::Peek => self.file_peek_subject().and_then(|it| it.path),
                    _ => self
                        .preview_picture(surface)
                        .map(|image| image.path.clone()),
                };
                let Some(path) = path else { continue };
                self.play_video_file_on(surface, &path)?;
                return Ok(true);
            }
            let Some(layout) = self.video_bar_layout_of(surface) else {
                // **No bar up, so the picture itself is the control** (user
                // ruling 2026-08-28: 「点 ▶ 或双击画面进播放态」). A double click
                // anywhere on a recording starts it, which is the gesture every
                // player has and the one a reader tries first.
                if self.double_click_started_a_video(surface, at)? {
                    return Ok(true);
                }
                continue;
            };
            if !layout.holds(at) {
                // The same gesture over a picture whose bar is up — and here it
                // is a *toggle*, because a video that is already playing is one
                // a second double click should stop.
                if self.double_click_started_a_video(surface, at)? {
                    return Ok(true);
                }
                continue;
            }
            let slot = layout.slot_at(at);
            let Some(key) = self
                .window
                .video
                .get(surface)
                .map(|seat| seat.key().to_owned())
            else {
                continue;
            };
            self.mouse_trace(|| {
                format!("video_bar press surface={surface:?} slot={slot:?} texture={key}")
            });
            let Some(seat) = self.window.video.get_mut(surface) else {
                continue;
            };
            match slot {
                Some(video_seat::BarSlot::PlayPause) => seat.toggle(now),
                Some(video_seat::BarSlot::Mute) => seat.toggle_mute(now),
                Some(video_seat::BarSlot::Rate) => seat.cycle_rate(now),
                Some(slot @ (video_seat::BarSlot::Seek | video_seat::BarSlot::Volume)) => {
                    seat.grab(slot, &layout, at, now);
                    self.window.video_bar_drag = Some(surface);
                }
                // The bar's own ground. Taken and not passed through: a press
                // that fell past a panel would land on the picture behind it,
                // and the picture's own gesture is a play.
                None => seat.acted(now),
            }
            self.refresh_chrome();
            self.present_chrome_change()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// **A double click on a recording's own picture** — start it, or stop it
    /// if it is already running. `true` when the press was the second of a pair
    /// and was taken.
    ///
    /// The picture and not a button, which is why it is here and not in
    /// [`Self::press_preview_image`]: that door is the *zoom*'s, and a video's
    /// still does not zoom (§7.23 ⑤) — it returns before it ever reaches a
    /// click. So the two gestures never meet, and a video's double click has to
    /// be counted where a video's presses are.
    ///
    /// [`ImageClicks`] is the same counter a picture's zoom toggle uses, and it
    /// is the same counter on purpose: what makes two presses a double click —
    /// the interval, and how far the pointer may have travelled between them —
    /// is a fact about a hand, not about what is under it.
    fn double_click_started_a_video(
        &mut self,
        surface: PreviewSurface,
        at: [f32; 2],
    ) -> Result<bool> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let now = Instant::now();
        let Some(shape) = self.video_shape_of(surface, scale, now) else {
            return Ok(false);
        };
        if !file_peek::contains(shape.rect(), at) {
            return Ok(false);
        }
        // Only over something that is a recording — a document, a picture and a
        // page all have their own answer to a double click.
        let playing = self.window.video.get(surface).is_some();
        if !playing {
            let names_a_video = match surface {
                PreviewSurface::Peek => self
                    .file_peek_subject()
                    .and_then(|it| it.path)
                    .is_some_and(|path| preview::path_names_a_video(&path)),
                _ => self
                    .preview_picture(surface)
                    .is_some_and(|image| preview::path_names_a_video(&image.path)),
            };
            if !names_a_video {
                return Ok(false);
            }
        }
        if !self.preview_image_clicks.register(surface, at, now) {
            // The first of a possible pair. Not taken: a single click on a
            // picture belongs to whatever the picture's own single click means.
            return Ok(false);
        }
        if playing {
            if let Some(seat) = self.window.video.get_mut(surface) {
                seat.toggle(now);
            }
            self.refresh_chrome();
            self.present_chrome_change()?;
            return Ok(true);
        }
        let path = match surface {
            PreviewSurface::Peek => self.file_peek_subject().and_then(|it| it.path),
            _ => self
                .preview_picture(surface)
                .map(|image| image.path.clone()),
        };
        let Some(path) = path else { return Ok(false) };
        self.play_video_file_on(surface, &path)?;
        Ok(true)
    }

    /// A held track follows the pointer, and it follows it **outside its own
    /// bar** — a scrub that stopped tracking the moment the hand left a
    /// thirty-four pixel strip would make the end of a recording a matter of
    /// aim. The same sentence every other drag in this window is written with.
    pub(crate) fn drag_video_bar(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(surface) = self.window.video_bar_drag else {
            return Ok(false);
        };
        let at = [position.x as f32, position.y as f32];
        let now = Instant::now();
        let Some(layout) = self.video_bar_layout_of(surface) else {
            return Ok(false);
        };
        if let Some(seat) = self.window.video.get_mut(surface) {
            seat.drag_to(&layout, at, now);
        }
        self.refresh_chrome();
        self.present_chrome_change()?;
        Ok(true)
    }

    /// **Tell every playing surface where the pointer is**, so the bar can come
    /// up under a hand that has settled and go away under one that has left.
    ///
    /// Told to *every* seat and not only to the one under the pointer, because
    /// "the pointer is somewhere else" is the fact that ends a reveal and a seat
    /// that was never told it would hold its bar up for ever.
    pub(crate) fn note_video_hover(&mut self, position: Option<PhysicalPosition<f64>>) {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let now = Instant::now();
        let at = position.map(|at| [at.x as f32, at.y as f32]);
        let shapes: Vec<(PreviewSurface, [f32; 4])> = self
            .window
            .video
            .iter()
            .map(|(surface, _)| surface)
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|surface| Some((surface, self.video_shape_of(surface, scale, now)?.rect())))
            .collect();
        let bars: Vec<(PreviewSurface, bool)> = shapes
            .iter()
            .map(|(surface, _)| {
                (
                    *surface,
                    at.is_some_and(|at| {
                        self.video_bar_layout_of(*surface)
                            .is_some_and(|layout| layout.holds(at))
                    }),
                )
            })
            .collect();
        for ((surface, rect), (_, over_bar)) in shapes.iter().zip(bars) {
            let Some(seat) = self.window.video.get_mut(*surface) else {
                continue;
            };
            match at.filter(|at| file_peek::contains(*rect, *at)) {
                Some(_) => seat.pointer_moved(over_bar, now),
                None => seat.pointer_left(),
            }
        }
    }

    /// **The control bar of every docked pane that is playing** (route B slice
    /// ②, 2026-08-28; §7.44 ②).
    ///
    /// One layer per playing pane, or none at all, which is the ordinary cost of
    /// this band on the overwhelming majority of frames: the map is empty and
    /// this is one iteration over nothing.
    pub(crate) fn preview_seat_video_bars(&self) -> Vec<marks::OverlayLayer> {
        let mut layers = Vec::new();
        for (surface, _) in self.window.video.iter() {
            if !matches!(surface, PreviewSurface::Seat(_)) {
                continue;
            }
            if let Some(layer) = self.video_bar_layer(surface) {
                layers.push(layer);
            }
        }
        layers
    }

    /// **One surface's control bar**, laid out against the very rectangle its
    /// picture is drawn in.
    ///
    /// [`Self::video_shape_of`] and not a second derivation, which is the whole
    /// reason that function hands back a struct: a bar computed from the pane's
    /// body while the picture is fitted to a tween's box would sit a few pixels
    /// off its own video for the length of every FLIP, and the two would agree
    /// again exactly when nothing was moving.
    pub(crate) fn video_bar_layer(&self, surface: PreviewSurface) -> Option<marks::OverlayLayer> {
        let seat = self.window.video.get(surface)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let now = Instant::now();
        let shape = self.video_shape_of(surface, scale, now)?;
        let layer = seat.bar(
            shape.rect(),
            scale,
            now,
            self.app.motion,
            &bt_render::chrome_palette(),
            self.video_meta_of(surface),
        );
        (!layer.quads.is_empty()).then_some(layer)
    }

    /// **Measure what every playing surface's bar says about its file** — once a
    /// frame, beside the font (owner's ruling 2026-09-12).
    ///
    /// The sentence is the same one the card says about the same recording
    /// ([`Self::video_meta_sentence`]); what this pass adds is the width it is
    /// drawn at, which decides where the speed button stands and is therefore
    /// what the hit test has to be resolved against.
    pub(crate) fn measure_video_meta(&mut self) {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let font = video_seat::bar_font_logical_px() * scale;
        let surfaces: Vec<PreviewSurface> = self
            .window
            .video
            .iter()
            .map(|(surface, _)| surface)
            .collect();
        let mut measured = std::collections::BTreeMap::new();
        for surface in surfaces {
            let Some(sentence) = self.video_meta_sentence(surface) else {
                continue;
            };
            let width =
                self.window
                    .renderer
                    .measure_chrome_text(&mut self.app.gpu, &sentence, font);
            measured.insert(surface, (sentence, width));
        }
        self.window.video_meta = measured;
    }

    /// What this frame measured for one surface — and nothing at all for a
    /// surface the pass has not reached yet, which is the honest answer for the
    /// first frame of a recording that has only just started playing.
    fn video_meta_of(&self, surface: PreviewSurface) -> video_seat::BarMeta<'_> {
        self.window.video_meta.get(&surface).map_or_else(
            video_seat::BarMeta::none,
            |(text, width)| video_seat::BarMeta {
                text: text.as_str(),
                width: *width,
            },
        )
    }

    pub(crate) fn preview_float_bar_layers(&self, id: float::FloatId) -> Vec<marks::OverlayLayer> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let surface = PreviewSurface::Float(id);
        let Some(body) = self.preview_surface_body_rect(surface, scale) else {
            return Vec::new();
        };
        self.preview_body_bar_layers(surface, body, scale)
    }

    /// The body bar the pointer is on, and the surface wearing it.
    ///
    /// **The card is not asked here.** It has its own press
    /// ([`Self::press_file_peek`]) and its own drag
    /// ([`Self::drag_file_peek_thumb`]), both of which run above every path that
    /// reaches this one, because the card is the topmost thing on the glass.
    fn preview_body_bar_under(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(PreviewSurface, preview::ScrollBar)> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let at = [position.x as f32, position.y as f32];
        let (surface, body) = self.preview_surface_at(position)?;
        // **The vertical one first**, which decides the one place the two can
        // both answer: the corner where a right-edge rule and a bottom-edge rule
        // cross, each grown sixteen pixels inward. Down is the gesture a hand
        // arriving at a scrollbar nearly always means, and giving the corner to
        // whichever happened to be asked first is how a corner becomes a
        // coin toss.
        let [across, down] = self.preview_surface_bars(surface, body, scale);
        [down, across]
            .into_iter()
            .flatten()
            .find(|bar| {
                bar.grab[0] <= at[0]
                    && at[0] <= bar.grab[2]
                    && bar.grab[1] <= at[1]
                    && at[1] <= bar.grab[3]
            })
            .map(|bar| (surface, bar))
    }

    /// A press on a body's scroll thumb takes hold of it.
    ///
    /// Returns whether the press was the thumb's. **Asked before the link and
    /// before a block's own thumb**, because this bar is the outermost piece of
    /// furniture the surface has: it is drawn over both of them, and a hit test
    /// that disagreed with the picture is the defect the block's bar already had
    /// once.
    pub(crate) fn press_preview_body_thumb(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some((surface, bar)) = self.preview_body_bar_under(position) else {
            return Ok(false);
        };
        // Along the bar's own axis, which is what makes the two one gesture:
        // the hand holds the thumb where it took hold of it, sideways or down.
        let (near, far) = match bar.axis {
            preview::ScrollAxis::Horizontal => (bar.thumb[0], bar.thumb[2]),
            preview::ScrollAxis::Vertical => (bar.thumb[1], bar.thumb[3]),
        };
        self.preview_body_drag = Some(PreviewBodyDrag {
            surface,
            axis: bar.axis,
            grab: (bar.along([position.x as f32, position.y as f32]) - near).clamp(0.0, far - near),
        });
        self.preview_body_hover = Some((surface, bar.axis));
        self.repaint_preview()?;
        Ok(true)
    }

    /// The pointer travelling with a body's thumb in hand.
    ///
    /// It keeps the pointer *outside* the pane too, which is every scrollbar's
    /// rule: a drag that stopped tracking the moment it left a seven-pixel band
    /// would make the end of a long file a matter of aim.
    pub(crate) fn drag_preview_body_thumb(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(drag) = self.preview_body_drag else {
            return Ok(false);
        };
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // The gesture's own surface, not the one under the pointer: a hand that
        // has left the pane it took hold in is still holding that pane's thumb.
        let Some(body) = self.preview_surface_body_rect(drag.surface, scale) else {
            return Ok(true);
        };
        let Some(bar) = self.preview_surface_bar(drag.surface, drag.axis, body, scale) else {
            // The document stopped overflowing — the pane grew, the file
            // changed. The gesture still owns the pointer until the button comes
            // up; there is simply nothing left for it to move.
            return Ok(true);
        };
        let along = bar.along([position.x as f32, position.y as f32]);
        let wanted = preview::scroll_dragged_to(&bar, along, drag.grab);
        let scroll = self.preview_pane_mut(drag.surface).scroll;
        let axis = usize::from(drag.axis == preview::ScrollAxis::Vertical);
        if (wanted - scroll[axis]).abs() < f32::EPSILON {
            return Ok(true);
        }
        let mut asked = scroll;
        asked[axis] = wanted;
        let scrolled = self.clamped_preview_scroll(drag.surface, body, scale, asked);
        self.preview_pane_mut(drag.surface).scroll = scrolled;
        self.repaint_preview()?;
        Ok(true)
    }

    /// Light the body thumb under the pointer, or put the last one out.
    pub(crate) fn note_preview_body_hover(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
    ) -> Result<()> {
        let over = position
            .and_then(|position| self.preview_body_bar_under(position))
            .map(|(surface, bar)| (surface, bar.axis));
        if over == self.preview_body_hover {
            return Ok(());
        }
        self.preview_body_hover = over;
        self.repaint_preview()
    }

    /// The markdown link under the pointer, if there is one, and the surface it
    /// is drawn on.
    fn preview_link_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(PreviewSurface, &PreviewLink)> {
        let (x, y) = (position.x as f32, position.y as f32);
        let (surface, _) = self.preview_surface_at(position)?;
        let link = self.preview_pane(surface)?.links.iter().find(|link| {
            x >= link.rect[0] && x <= link.rect[2] && y >= link.rect[1] && y <= link.rect[3]
        })?;
        Some((surface, link))
    }

    /// **Go where a markdown link points** (user ruling, 2026-08-13).
    ///
    /// Reached from the **release** and not from the press (user report
    /// 2026-08-28). It used to be a press, on the argument that a link is a
    /// control standing in the content and a press on a control was never a
    /// press on what it stands on — which was true for as long as the prose
    /// underneath answered nothing. Now it answers a drag, and the two verbs
    /// have to share one button: a press that held still is the link's and a
    /// press that travelled is the selection's, which is exactly the
    /// `click_no_drag` gate the terminal's own hyperlinks are opened through
    /// ([`hyperlink_activation`]). See [`preview_press_opens_its_link`].
    ///
    /// **And it reads the modifier since 2026-08-29** (§7.1.5g ⑦). It did not,
    /// for the whole of its life before that day: a web address in a document
    /// went to `ShellExecuteW` on a plain press, which is the answer §7.1.5g
    /// settled *against* four months later and never came back here to change.
    /// The modifier is [`PreviewTextDrag::control`], recorded when the button
    /// went down for [`SelectionDrag::hyperlink_control`]'s own reason: a `Ctrl`
    /// let go of during a click must not change the destination underneath a
    /// gesture already begun.
    fn open_preview_link(
        &mut self,
        surface: PreviewSurface,
        target: &str,
        control: bool,
    ) -> Result<bool> {
        // A relative link is resolved against the document's **own folder**, so
        // a document that is not in a folder cannot resolve one.
        let Some(document) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.source.file_path())
            .map(Path::to_path_buf)
        else {
            return Ok(false);
        };
        match preview_link_activation(control, target, &document) {
            // §7.1.3's one door, the same one the tree's Enter and the file
            // menu's first row go through — so a file reached by pointing at it
            // in prose lands exactly where a file reached any other way does,
            // pool and all. Anything unreadable arrives as the "no preview"
            // card, which carries its own way out to the system.
            PreviewLinkActivation::Preview(path) => self.open_preview(path)?,
            // **The same address, kept in this window** — the terminal's own
            // `Page` arm, spent through the terminal's own door
            // ([`Self::open_web_address_here`]). A refusal is said out loud
            // because a request was made, and it is said *here*, on the surface
            // the press landed on: the terminal's mouth for this is a status
            // line under the cells the address is printed in, and a link inside
            // a document has no cells.
            PreviewLinkActivation::Page(url) => {
                if !self.open_web_address_here(&url)? {
                    self.say_address_refused(surface, &url)?;
                }
            }
            PreviewLinkActivation::Browser(url) => {
                let result = native_window(&self.window.window).and_then(|native| {
                    bt_platform::shell_execute(native, &url)
                        .map_err(|error| anyhow!(error))
                        .context("open a markdown link in the system browser")
                });
                if let Err(error) = result {
                    eprintln!("recoverable markdown link open failure: {error:#}");
                }
            }
            PreviewLinkActivation::Blocked(url) => self.say_address_refused(surface, &url)?,
            // A scheme this window does not open, or an anchor it cannot yet
            // honour. The press is still the link's: it landed on a control,
            // and letting it fall through would put a caret in the prose.
            PreviewLinkActivation::None => {}
        }
        Ok(true)
    }

    /// **Which byte of a rendered page a point names**, in the document's own
    /// terms.
    ///
    /// `preview_offset_at`'s opposite number on the other face of the same
    /// surface, and the difference between them is the whole reason this
    /// feature needed writing: that one divides by a cell width, because the
    /// source face is a monospace grid; a rendered page is proportional text
    /// that has already wrapped, so where a letter is on screen is a question
    /// only the shaper can answer — the same shaper that drew it.
    fn preview_place_at(
        &mut self,
        surface: PreviewSurface,
        position: PhysicalPosition<f64>,
    ) -> Option<preview_select::Place> {
        let (x, y) = (position.x as f32, position.y as f32);
        // Cloned out so the pane's borrow ends before the shaper's begins. One
        // paragraph per pointer event, which is nothing beside the shaping the
        // same event's repaint is about to do.
        let hit = {
            let boxes = &self.preview_pane(surface)?.md_text;
            let at = preview_text_box_at(boxes, x, y)?;
            boxes[at].clone()
        };
        let Some(paragraph) = hit.paragraph else {
            // A picture: there are no letters to stand between, and the piece is
            // an atom anyway.
            return Some(hit.piece.at);
        };
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let shaped = renderer.measure_preview_hit(gpu, &paragraph, x, y);
        Some(preview_select::Place {
            offset: hit.piece.doc_offset(shaped),
            ..hit.piece.at
        })
    }

    /// **The box the source block is drawn in**, when this surface is drawing
    /// one (T5, §7.1.3t).
    ///
    /// The layout pass's own arithmetic, read back a frame later: the measure
    /// box for the sides, and the block's `top` out of the layout, lifted into
    /// window pixels by the body's origin and this surface's scroll. Said here
    /// rather than at the three call sites — the hit test, the reveal and the
    /// composition's caret box — because a source block hit-tested in one
    /// rectangle and drawn in another is a caret that lands where nobody
    /// pointed.
    ///
    /// A caret's block never scrolls sideways inside itself — the monospace one
    /// folds on the source face's own terms (§7.1.3q) and the prose one wraps
    /// like any paragraph (§7.1.3w) — so unlike a table or a fence its box is
    /// the page's column and no block offset is subtracted from it.
    fn markdown_caret_box(
        &self,
        surface: PreviewSurface,
        scale: f32,
    ) -> Option<([f32; 4], &MarkdownCaretBlock, MarkdownBlockLayout)> {
        let body = self.preview_surface_body_rect(surface, scale)?;
        let metrics = seats::preview_markdown_metrics(scale);
        let (left, right) = preview::markdown_measure_box(body, metrics);
        let pane = self.preview_pane(surface)?;
        let PreviewDocument::Markdown { source, layout, .. } = &pane.doc else {
            return None;
        };
        let source = source.as_deref()?;
        let placed = layout.get(source.index())?;
        let top = body[1] + metrics.padding_y - pane.scroll[1] + placed.top;
        Some((
            [left, top, right.max(left), top + placed.height],
            source,
            placed,
        ))
    }

    /// The monospace block's box, when the caret's block wears that face.
    fn markdown_source_box(
        &self,
        surface: PreviewSurface,
        scale: f32,
    ) -> Option<([f32; 4], &MarkdownSourceBlock)> {
        let (box_of_block, block, _) = self.markdown_caret_box(surface, scale)?;
        Some((box_of_block, block.mono()?))
    }

    /// **Which byte of the file a point in a rendered markdown page names**
    /// (T5 ①) — the two halves of one question, in the order that makes them one
    /// answer.
    ///
    /// **The source block is hit-tested first**, and that is not an
    /// optimisation. It pushes no [`PreviewTextSite`]s — it is monospace rows
    /// and not shaped prose, so there is no piece of a parse under it — which
    /// means [`preview_text_box_at`] would answer a press inside it with the
    /// nearest box that *is* a piece: the paragraph above or below. Clicking
    /// into the block you are editing would put the caret in its neighbour.
    ///
    /// Only the rows are asked about and not the columns, for
    /// [`preview_text_box_at`]'s own reason one block along: the margin either
    /// side of the column of prose belongs to the row it is beside, so a click
    /// out at the edge of the pane lands at the end of the line it is level
    /// with.
    ///
    /// Everything else goes through the rendered page's own hit test and then
    /// through T6's provenance (§7.1.3r), which is what makes a click land on
    /// the word it was aimed at rather than at the top of its paragraph.
    ///
    /// **An empty document is the one page with no piece to land on** (user
    /// report, 2026-09-11: a file just made by `New file…` could not be typed
    /// into). Nothing was parsed, so there is no block, no box and no
    /// provenance to ask — and a press that named nothing was read as a press on
    /// the page's empty ground, which leaves the page rather than entering it.
    /// A file with no bytes has exactly one place a caret can be, so a press
    /// anywhere in its body is that place: the empty line the page already draws
    /// the caret on ([`preview_live`]'s gap with no block in front of it).
    fn preview_md_file_offset_at(
        &mut self,
        surface: PreviewSurface,
        position: PhysicalPosition<f64>,
    ) -> Option<usize> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (x, y) = (position.x as f32, position.y as f32);
        if let Some((box_of_block, source)) = self.markdown_source_box(surface, scale)
            && y >= box_of_block[1]
            && y < box_of_block[3]
        {
            return Some(markdown_source_offset_at(source, box_of_block, x, y));
        }
        // **And the prose block is hit-tested first for the same reason**
        // (§7.1.3w). It pushes no [`PreviewTextSite`]s either — what it draws is
        // the file's own bytes and not a piece of the parse, so there is no
        // provenance under it to ask — and the geometry it is drawn in is the
        // geometry it is read back in: the nearest seam to the pointer, which is
        // the very seam the caret will be struck at.
        if let Some(prose) = self
            .preview_pane(surface)
            .and_then(|pane| pane.md_prose.as_ref())
            && let (Some(first), Some(last)) = (prose.rows.first(), prose.rows.last())
            && y >= first.top
            && y < last.top + last.height
        {
            return Some(prose.press(x, y));
        }
        if let Some(body) = self.preview_surface_body_rect(surface, scale)
            && let Some(pane) = self.preview_pane(surface)
            && let Some(offset) = markdown_empty_page_offset(&pane.doc, body, x, y)
        {
            return Some(offset);
        }
        let place = self.preview_place_at(surface, position)?;
        let PreviewDocument::Markdown {
            blocks,
            ranges,
            maps,
            ..
        } = &self.preview_pane(surface)?.doc
        else {
            return None;
        };
        preview_provenance::file_offset_of(&place, blocks, ranges, maps)
    }

    /// The rendered page under the pointer, if the pointer is over one.
    ///
    /// A *rendered* page and not merely a preview surface: the source face has
    /// its own selection ([`preview_edit::EditCaret`]) and a picture has none,
    /// so this is the question "is there a document on the glass here whose
    /// words can be picked up".
    pub(crate) fn preview_rendered_surface_at(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<PreviewSurface> {
        let (surface, _) = self.preview_surface_at(position)?;
        matches!(
            self.preview_pane(surface)?.doc,
            PreviewDocument::Markdown { .. }
        )
        .then_some(surface)
    }

    /// **A press inside a rendered page** (user report 2026-08-28: 「渲染后的
    /// md 文字无法选中」).
    ///
    /// It takes the press whole, and it takes the press a link landed on too —
    /// see [`Self::open_preview_link`] for why the link now answers at the
    /// release instead. What it does *not* take is a press on any of the
    /// furniture standing over the page: the body's bar, a wide block's bar and
    /// a picture are all asked before it, exactly as they were before there was
    /// anything under them to select.
    ///
    /// **And it changes nothing the reader can see** (owner's report and ruling
    /// 2026-09-21, `docs/DESIGN.md`'s entry of that date). It used to put the
    /// caret in the page where it landed, so the block under the pointer put its
    /// marks back and re-flowed the instant the button went down, and went on
    /// changing shape under a hand still drawing a selection across it. What it
    /// does now is **record** what it would do ([`preview_press::Pressed`]) and
    /// arm the drag; [`Self::release_preview_text`] spends the record. The page
    /// keeps the face it had, and what the gesture draws in the meantime is the
    /// page's own pieces — the model that needs no block to be source.
    pub(crate) fn press_preview_text(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(surface) = self.preview_rendered_surface_at(position) else {
            return Ok(false);
        };
        let shift = self.window.modifiers.shift_key();
        let link = self
            .preview_link_at(position)
            .map(|(_, link)| link.target.clone());
        // **The third mouth on this press** (T5 ①, §7.1.3t). The other two are
        // already here — a press that travels draws a selection, and a press
        // that does not opens the link it landed on — and the one this ticket
        // adds is the plainest of the three: a press in the body of a document
        // you can type into puts the caret where the pointer is. There is no
        // pencil to press first and no double click to earn it, because a
        // document you can type into is one you type into.
        //
        // **A plain press on a link is still the link's**, which is the one
        // exception and is what the six-pixel latch is for: the reader is
        // following it, not aiming at the letters inside it. A *shift* press is
        // an extension whatever it is standing over, so it is the caret's even
        // on a link — the run being drawn across may well contain one.
        let live = self.preview_shows_live_markdown(surface);
        let takes_a_caret = self.preview_caret_takes_the_press(surface);
        let caret_at = takes_a_caret
            .then(|| self.preview_md_file_offset_at(surface, position))
            .flatten();
        let placing = caret_at.filter(|_| link.is_none() || shift);
        let point = [position.x as f32, position.y as f32];
        let clicks = self
            .preview_text_clicks
            .register(surface, point, Instant::now());
        let grain = preview_text_grain(clicks);
        // **The record, and it is the whole of what this press does to the
        // page** (owner's ruling 2026-09-21). One of two things is named — a
        // byte of the file, or none at all, which is the page's empty ground
        // (owner's ruling 2026-09-10) — and neither is acted on until the
        // gesture ends.
        //
        // **A press on a link is not empty ground**, which is why the second arm
        // asks `caret_at` and not `placing`: the link's own press leaves
        // `placing` empty while standing on letters the file spells. It records
        // nothing, because it has no caret to place; so does a press on a page
        // nobody can type into, which is a reader's and draws pieces.
        let pressed = match placing {
            Some(offset) => Some(preview_press::Pressed::byte(offset, grain, shift)),
            None if live && caret_at.is_none() => Some(preview_press::Pressed::Ground),
            None => None,
        };
        // Shift-click extends from wherever the selection already was, which is
        // the quick edit's own gesture and the one that makes a long selection
        // possible without a drag that outruns the pane.
        let standing = self
            .preview_pane(surface)
            .and_then(|pane| pane.md_select)
            .filter(|_| shift);
        // Asked before anything moves, because it is a question about the page
        // as the hand found it.
        let in_the_seat = self.preview_press_keeps_the_caret_seat(surface, caret_at);
        if pressed.is_some() && !shift {
            // **A plain press lets go of what was selected**, and that is the
            // one thing left that happens at once: dropping a highlight moves no
            // caret and turns no block into source, and a selection the last
            // gesture left standing would otherwise go on being drawn over the
            // one this gesture is drawing (the caret answers before the pieces
            // do, everywhere).
            self.drop_preview_selection(surface);
        }
        // **Every gesture draws the page's own pieces now** (this ruling): the
        // caret has not moved, so the caret model has nothing to draw, and the
        // piece model is the one that needs no block to be source. It is also
        // the model a page with no caret in it has always used — a document this
        // window will not edit, truncated, lossy, over the cap, a diff — so a
        // reader's drag and an editor's draw the same way while they are in
        // flight (research §10 Q3).
        //
        // **Except inside the seat the page is already drawing from**, which is
        // the one patch of a live page with no pieces under it: it is drawn from
        // the file's own bytes and pushes no [`PreviewTextSite`]s, so the
        // nearest piece to a press inside it is in the paragraph above or below.
        // A gesture there draws the caret's own band instead, which is what the
        // spend two paragraphs down is for.
        let mut drew = false;
        if !in_the_seat && let Some(place) = self.preview_place_at(surface, position) {
            let selection = match standing {
                Some(was) => preview_select::Selection { head: place, ..was },
                None => preview_select::Selection::collapsed(place, grain),
            };
            self.preview_pane_mut(surface).md_select = Some(selection);
            drew = true;
        }
        // **And the one press that is answered where it stands** (closure review
        // of this ruling, 2026-09-21): the page's face is a function of the
        // caret's seat alone, so a caret moving *inside* the seat already drawn
        // changes nothing and there is nothing to wait for. Waiting would cost
        // the commonest editing gesture its highlight — dragging across the
        // words you are about to replace, in the paragraph you are already
        // editing, with no band under the hand until it let go.
        //
        // The record is kept all the same, and the release spends it again: the
        // spend is one function of what the press named, so spending it twice
        // leaves exactly what spending it once does.
        let seated = in_the_seat && pressed.is_some();
        if in_the_seat && let Some(pressed) = pressed {
            self.spend_preview_press(surface, pressed.spend(false, None))?;
        }
        if pressed.is_none() && link.is_none() && !drew {
            // Nothing to spend, nothing drawn and no link to follow: the press
            // is not this page's, and the ladder goes on underneath it.
            return Ok(false);
        }
        self.preview_text_drag = Some(PreviewTextDrag {
            surface,
            // A shift-click has already extended the selection and must not have
            // to travel to keep it; the latch is what decides whether the *link*
            // answers, and a shift-click on one is not asking for the link.
            latch: DragLatch::new(position),
            link: link.filter(|_| standing.is_none() && placing.is_none()),
            // **The hand-over modifier, not `Ctrl` by name** (§13.45 ①): `Ctrl`
            // here, `⌘` on a Mac, read off what the hand is holding.
            control: input::pointer_chord_held(self.window.modifiers_held),
            pressed,
            seated,
            reached: None,
        });
        self.repaint_preview()?;
        Ok(true)
    }

    /// **A plain press lets go of what was selected**, on whichever of the two
    /// models was holding it (owner's ruling 2026-09-21).
    ///
    /// The caret itself does not move — its anchor comes to it, which is what
    /// "nothing is selected" means ([`preview_edit::EditCaret`]) — so the block
    /// drawn as source is the block that was already drawn as source. That is
    /// what lets this happen at the press while everything else waits for the
    /// release: a highlight going out is not the page changing shape.
    ///
    /// A *shift*-press is an extension and never reaches here: what it extends
    /// from is the very anchor this would drop.
    pub(crate) fn drop_preview_selection(&mut self, surface: PreviewSurface) {
        let pane = self.preview_pane_mut(surface);
        pane.caret.anchor = pane.caret.caret;
        pane.md_select = None;
    }

    /// **Whether a byte of the file is in the seat this page is already drawing
    /// from** ([`preview_press::keeps_the_seat`], asked of a surface).
    ///
    /// Two questions turn on it and they are one question. A press there may be
    /// answered where it stands, because putting the caret inside the seat the
    /// page is already drawing changes no block's face; and a gesture there has
    /// no rendered pieces under it, because what the seat is drawn as is the
    /// file's own bytes and it pushes no [`PreviewTextSite`]s.
    pub(crate) fn preview_press_keeps_the_caret_seat(
        &self,
        surface: PreviewSurface,
        offset: Option<usize>,
    ) -> bool {
        let Some(offset) = offset else {
            return false;
        };
        let Some(content) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.as_deref())
        else {
            return false;
        };
        let Some(PreviewDocument::Markdown { ranges, .. }) =
            self.preview_pane(surface).map(|pane| &pane.doc)
        else {
            return false;
        };
        preview_press::keeps_the_seat(
            content,
            ranges,
            self.preview_live_caret(surface).map(|caret| caret.caret),
            offset,
        )
    }

    /// **Whether a press on this surface may put a caret in the page** (T5 ①).
    ///
    /// Three answers folded into one so that the press does not have to hold
    /// three: the page edits, and the caret lands now; the page is waiting for
    /// the whole file it has just been asked to buy, and the caret lands when
    /// the body does ([`Self::settle_preview_caret`]); or neither, and the press
    /// is a reader's.
    ///
    /// **The glance card is never one of the three.** It is read-only by its
    /// founding ruling — a thumbnail must not ask the disk — and
    /// [`Self::preview_surfaces`] never lists it, so a caret placed on it could
    /// never be typed into and would be a caret drawn where no keystroke goes.
    fn preview_caret_takes_the_press(&self, surface: PreviewSurface) -> bool {
        if matches!(surface, PreviewSurface::Peek) {
            return false;
        }
        self.preview_shows_live_markdown(surface) || self.preview_awaits_the_whole_file(surface)
    }

    /// Whether this Markdown surface is still owed a complete load result —
    /// [`preview::PreviewBuffer::awaits_the_whole_file`] asked of the face too.
    fn preview_awaits_the_whole_file(&self, surface: PreviewSurface) -> bool {
        if self.preview_md_source(surface) {
            return false;
        }
        self.preview_buffer_on(surface).is_some_and(|buffer| {
            buffer.view(false) == preview::PreviewView::Markdown && buffer.awaits_the_whole_file()
        })
    }

    /// **Put the caret in the rendered page** — the whole of entering
    /// (§7.1.3t).
    ///
    /// Five writes, and they are one gesture: the caret moves, the page's other
    /// selection model lets go, the page starts drawing the caret's block as
    /// source, the press that was waiting for a body is spent, and the keyboard
    /// arrives. One door because every one of them is what "there is a caret in
    /// this page" means, and a caller that remembered four of the five would
    /// leave a surface drawing a source block nothing can be typed into.
    ///
    /// **`md_select` goes** (research §10 Q3, ruled): two anchors on one surface
    /// are two answers to "what is selected", so the moment the caret becomes
    /// the model the piece selection stops being one.
    fn place_preview_caret_on(
        &mut self,
        surface: PreviewSurface,
        offset: usize,
        extend: bool,
    ) -> Result<()> {
        if !self.seat_preview_caret(surface, offset, extend) {
            return Ok(());
        }
        self.repaint_preview()
    }

    /// **A repeated press takes more than a character** (T5 ①, §7.31 ⑦'s grain
    /// carried onto the caret).
    ///
    /// The word is `preview_select`'s own — one classifier, so a double click in
    /// a rendered paragraph takes exactly what a double click in the terminal
    /// beside it takes — walked over the *file's* bytes rather than a piece's,
    /// because that is the coordinate the caret is in.
    ///
    /// A triple click takes the **block**, which is the caret model's answer to
    /// `Grain::Piece` and is the same thing said in file bytes: a paragraph on
    /// this page has no lines of its own, so what a third press can honestly
    /// take is the run of the file the block was parsed from — its trailing
    /// break off, because the blank line after a paragraph is nobody's
    /// (§7.1.3o). In a gap, where no block was parsed from, the file's own line
    /// is what there is.
    fn widen_preview_caret(
        &mut self,
        surface: PreviewSurface,
        grain: preview_select::Grain,
    ) -> Result<()> {
        if grain == preview_select::Grain::Character {
            return Ok(());
        }
        let Some(content) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.clone())
        else {
            return Ok(());
        };
        let Some(at) = self.preview_pane(surface).map(|pane| pane.caret.caret) else {
            return Ok(());
        };
        let range = match grain {
            preview_select::Grain::Word => {
                preview_select::word_start(&content, at)..preview_select::word_end(&content, at)
            }
            _ => match self.standing_block_range(surface, at) {
                Some(range) => {
                    range.start..range.start + preview_live::block_source(&content, &range).len()
                }
                None => {
                    let starts = preview_edit::line_starts(&content);
                    let line = preview_edit::line_index(&starts, at);
                    let (from, to) = preview_edit::line_bounds(&content, &starts, line);
                    from..to
                }
            },
        };
        if range.is_empty() {
            return Ok(());
        }
        let pane = self.preview_pane_mut(surface);
        pane.caret.anchor = preview_edit::normalize(&content, range.start);
        pane.caret.caret = preview_edit::normalize(&content, range.end);
        pane.caret.desired_column = None;
        self.repaint_preview()
    }

    /// The five writes on their own, with no frame asked for.
    ///
    /// Split from [`Self::place_preview_caret_on`] for exactly one caller:
    /// [`Self::settle_preview_caret`] runs *inside* the layout refresh, which is
    /// about to rebuild the body anyway, and a repaint asked for from in there
    /// would be the refresh calling itself.
    ///
    /// Reports whether anything was written, which is `false` only for a surface
    /// whose bytes are not in hand — and then the caret has nothing to be an
    /// offset into.
    fn seat_preview_caret(&mut self, surface: PreviewSurface, offset: usize, extend: bool) -> bool {
        let Some(content) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.clone())
        else {
            return false;
        };
        let pane = self.preview_pane_mut(surface);
        // A shift-press extends only from a caret that was already standing in
        // this page: the first press into a page has nothing to extend from.
        let extend = extend && pane.md_caret;
        let mut caret = pane.caret;
        caret.place(&content, offset, extend);
        pane.caret = caret;
        pane.md_select = None;
        pane.md_caret = true;
        pane.md_caret_wanted = None;
        self.preview_edit_focus = Some(surface);
        self.reveal_preview_caret(surface);
        true
    }

    /// **Take the caret out of the rendered page** — the whole of leaving, and
    /// the mirror of [`Self::seat_preview_caret`] (T5 ③, §7.1.3t; owner's
    /// ruling 2026-09-10).
    ///
    /// The page renders again and the keyboard goes back. **The caret itself is
    /// kept**: it is a byte offset into the buffer, so the next press back into
    /// the page — and the flip to the source face, and the arrow key that
    /// follows — finds it exactly where this left it.
    ///
    /// **Three gestures reach it, and they are one sentence**: `Esc`, a press on
    /// the page's empty ground, and this surface losing the keyboard to a press
    /// somewhere else. The earlier ruling had only the first of the three — a
    /// page that re-flowed itself every time a hand left it would move while
    /// nobody was looking at it — and the owner reversed it on the report that
    /// followed: a document left with one paragraph still wearing its markup
    /// does not read as a document, and a reader who has clicked away has
    /// finished with it as plainly as one who pressed `Esc`. The two halves that
    /// argument turns on are both kept: what the page *draws* goes back to
    /// prose, and where the caret *is* does not move.
    pub(crate) fn leave_preview_page(&mut self, surface: PreviewSurface) {
        self.preview_pane_mut(surface).md_caret = false;
        self.preview_pane_mut(surface).md_caret_wanted = None;
        if self.preview_edit_focus == Some(surface) {
            self.preview_edit_focus = None;
        }
    }

    /// The pointer travelling with the button down, drawing across a page.
    ///
    /// It owns the pointer **outside** the page it began on, for the reason the
    /// quick edit's own drag does: a selection that stopped extending the moment
    /// the hand left the pane would make selecting the last line a matter of aim.
    ///
    /// **The caret may move while the seat does not, and no further** (owner's
    /// ruling 2026-09-21, as its closure review narrowed it). It used to follow
    /// the hand wherever it went, and that is half of the report this ruling
    /// answers: a caret dragged through a document takes the source block with
    /// it, so every paragraph the hand crossed put its marks back and re-flowed
    /// as the pointer arrived.
    ///
    /// So a gesture that began inside the seat the page was already drawing
    /// ([`PreviewTextDrag::seated`]) goes on extending the caret for as long as
    /// the pointer is in that seat — nothing can change face, and an editing
    /// drag has to show what it is taking. Past the edge of the seat the caret
    /// stops, and what the drag does from there is remember the byte it reached
    /// for [`Self::release_preview_text`] to spend. Every other gesture extends
    /// the rendered selection being drawn over the page as it stands.
    pub(crate) fn drag_preview_text(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let scale = self.window.renderer.metrics().scale_factor;
        let Some(drag) = self.preview_text_drag.as_mut() else {
            return Ok(false);
        };
        let surface = drag.surface;
        let crossed = drag.latch.travelled(position, scale);
        let begun = drag.latch.begun;
        let spends = drag.pressed.is_some();
        let seated = drag.seated;
        if crossed {
            // A press that travelled is not half of a double click — J99's rule,
            // at this window's third double-click surface.
            self.preview_text_clicks.interrupt();
        }
        if !begun {
            // Still a click. Nothing has been selected yet, so nothing is drawn
            // — the six pixels are what keep a press meant for a link from
            // flashing a character of highlight under the hand.
            return Ok(true);
        }
        // **Where the gesture has got to, in the file's own bytes**, kept
        // against the button coming up somewhere the page has no byte to name:
        // off the bottom of the pane, which is how the last line of a document
        // is selected.
        if spends && let Some(offset) = self.preview_md_file_offset_at(surface, position) {
            if let Some(drag) = self.preview_text_drag.as_mut() {
                drag.reached = Some(offset);
            }
            // **The caret follows the hand while the seat holds** — the band
            // under an editing drag, drawn as it is drawn. The seat is asked
            // again on every report rather than remembered, because it is the
            // same question the press asked and the caret has not left it: a
            // page whose face cannot change is one this may write to.
            if seated && self.preview_press_keeps_the_caret_seat(surface, Some(offset)) {
                if self.preview_pane(surface).map(|pane| pane.caret.caret) != Some(offset) {
                    self.place_preview_caret_on(surface, offset, true)?;
                }
                return Ok(true);
            }
        }
        let Some(place) = self.preview_place_at(surface, position) else {
            return Ok(true);
        };
        let was = self.preview_pane(surface).and_then(|pane| pane.md_select);
        let Some(selection) = was else {
            return Ok(true);
        };
        if selection.head == place {
            return Ok(true);
        }
        self.preview_pane_mut(surface).md_select = Some(preview_select::Selection {
            head: place,
            ..selection
        });
        self.repaint_preview()?;
        Ok(true)
    }

    /// **The button coming up on a rendered page.**
    ///
    /// Three outcomes and one gate between them: a press that never travelled is
    /// a click — it opens the link it landed on, if it landed on one, and lets
    /// go of whatever was selected either way — and a press that travelled is a
    /// selection, which `Copy on select` may put on the clipboard.
    ///
    /// **And it is where the press is finally answered** (owner's ruling
    /// 2026-09-21): the gesture is over, so the page may change now. The record
    /// is spent above the gate and not inside it, because the gate is about the
    /// *link* and the caret is owed whichever side of it this gesture falls —
    /// the same moment either way, which is what leaves 点=跟随链接、拖=选中
    /// exactly as it was.
    pub(crate) fn release_preview_text(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(drag) = self.preview_text_drag.take() else {
            return Ok(false);
        };
        let click = preview_press_opens_its_link(&drag.latch);
        if let Some(pressed) = drag.pressed {
            // Where the hand let go, or — for a button that came up off the page
            // — the last byte the drag reached. A click spends neither.
            let head = (!click)
                .then(|| {
                    self.preview_md_file_offset_at(drag.surface, position)
                        .or(drag.reached)
                })
                .flatten();
            self.spend_preview_press(drag.surface, pressed.spend(!click, head))?;
        }
        // Asked of the *bytes* and not of the gesture, which is what keeps the
        // two presses that select without travelling: a double click has taken a
        // word and a shift-click has taken a stretch, and neither of them let go
        // of it by finishing.
        let selected = self.preview_selected_text(drag.surface).is_some();
        if click {
            // **A click on blank glass lets go of the selection**, which is what
            // every reader expects of a click in a page of text — and the same
            // click on a link is still the link's.
            if !selected {
                self.preview_pane_mut(drag.surface).md_select = None;
            }
            self.repaint_preview()?;
            if let Some(target) = drag.link {
                self.open_preview_link(drag.surface, &target, drag.control)?;
                // A press that has just been spent on a link is not a selection
                // to write, whatever it was standing over.
                return Ok(true);
            }
        }
        if preview_copies_on_select(self.app.settings_store.loaded().copy_on_select, selected) {
            // **The selection stays standing**, which is copy-on-select's own
            // rule next door: the reader has not asked for it to go away, and a
            // highlight that vanished the instant the button came up would be a
            // highlight nobody could check.
            self.copy_preview_text_selection(drag.surface);
        }
        Ok(true)
    }

    /// **What the press recorded, spent now that the gesture is over** (owner's
    /// ruling 2026-09-21) — the three answers [`Self::press_preview_text`] used
    /// to give itself, given one gesture later and in one place.
    ///
    /// One door rather than three arms at the release, for
    /// [`Self::seat_preview_caret`]'s own reason: what a press on this page
    /// means is a sentence, and a second caller that spelled two thirds of it
    /// would be a second answer to "when does a page change face".
    ///
    /// The three are unchanged in meaning. A page that edits takes the caret,
    /// grown to the grain a repeated press asked for and then drawn out to where
    /// the hand let go. **A page still waiting for the file it has just been
    /// asked to buy** keeps the offset instead (T2 ③, T5 ①): the read is a read,
    /// and the alternative is a reader clicking, watching nothing happen, and
    /// clicking again — so the byte waits in `md_caret_wanted` until
    /// [`Self::settle_preview_caret`] has a body to put it in. **A press that
    /// named no byte of the file** takes the reader back out of the page
    /// (owner's ruling 2026-09-10), keeping the caret where it stands.
    pub(crate) fn spend_preview_press(
        &mut self,
        surface: PreviewSurface,
        spend: preview_press::Spend,
    ) -> Result<()> {
        match spend {
            preview_press::Spend::Ground => {
                if self.preview_shows_live_markdown(surface) {
                    self.leave_preview_page(surface);
                    self.repaint_preview()?;
                }
            }
            preview_press::Spend::Caret {
                offset,
                grain,
                extend,
                head,
            } => {
                if self.preview_shows_live_markdown(surface) {
                    self.place_preview_caret_on(surface, offset, extend)?;
                    // **And the grain of a repeated press survives the new
                    // model** (§7.31 ⑦, said in the caret's own coordinate): a
                    // double click in a rendered page takes a word and a triple
                    // click takes a paragraph, exactly as they did before this
                    // page could be typed into. A gesture that travelled is
                    // drawn out from what the grain took, which is what keeps a
                    // double click's word whole when the hand carries on.
                    self.widen_preview_caret(surface, grain)?;
                    if let Some(head) = head {
                        self.place_preview_caret_on(surface, head, true)?;
                    }
                } else {
                    self.preview_pane_mut(surface).md_caret_wanted = Some(offset);
                }
            }
        }
        Ok(())
    }

    /// Light the link under the pointer, or put the last one out.
    pub(crate) fn note_preview_link_hover(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
    ) -> Result<()> {
        let over = position
            .and_then(|position| self.preview_link_at(position))
            .map(|(surface, link)| (surface, link.clone()));
        if over == self.preview_link_hover {
            return Ok(());
        }
        self.preview_link_hover = over;
        self.apply_pointer_cursor();
        self.repaint_preview()
    }

    /// The colour token under the pointer, and the box it is drawn in
    /// (§7.1.6c-4c).
    ///
    /// **The painter's arithmetic read forwards**, where `preview_offset_at`
    /// reads it backwards: the pointer names an offset, the offset names a line
    /// and a token in it, and the token's columns name the box it occupies on
    /// the row it was wrapped onto. Both directions go through the same
    /// `WrapLayout`, so a reflowed line answers with the box the reader can see
    /// rather than the one an unwrapped document would have had.
    ///
    /// The last step is a guard rather than a computation: the offset arrives
    /// clamped to the row's last column, so a pointer resting past the end of a
    /// line whose last token is a colour would otherwise be told it is inside
    /// that colour. Asking whether the pointer is inside the box that was just
    /// computed is the general form of that check, and it costs one comparison.
    fn preview_hex_at(&self, position: PhysicalPosition<f64>) -> Option<PreviewHexHover> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (surface, body) = self.preview_surface_at(position)?;
        let content = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.as_deref())?;
        let offset = self.preview_offset_at(surface, body, scale, position)?;
        let starts = self.preview_buffer_on(surface)?.line_starts();
        let line_index = preview_edit::line_index(starts, offset);
        let (from, to) = preview_edit::line_bounds(content, starts, line_index);
        let line = &content[from..to];
        let token = hex_peek::hex_token_at(line, offset.saturating_sub(from))?;

        let pane = self.preview_pane(surface)?;
        let metrics = seats::preview_text_metrics(scale);
        let advance = pane.mono_advance;
        if advance <= 0.0 {
            return None;
        }
        let wrap = self.preview_wrap(surface)?;
        let start_column = preview_edit::column_of(line, token.range.start);
        let end_column = preview_edit::column_of(line, token.range.end);
        // `row_of` answers with the row **and the column that row starts at** —
        // not with the column inside it — which is what the caret needs and what
        // a box has to subtract.
        let (row, row_start) = wrap.row_of(line_index, start_column);
        let (end_row, _) = wrap.row_of(line_index, end_column);
        let column = start_column.saturating_sub(row_start);
        // A token the wrap broke across two rows is anchored on the part of it
        // that begins the break, cut at that row's own end: a box running from
        // one row's middle to the next row's middle is not a box the token is
        // drawn in. `row_span` is what says where the row ends.
        let row_end = if end_row == row {
            end_column
        } else {
            wrap.row_span(row).map_or(end_column, |(_, _, to)| to)
        };
        let columns = row_end.saturating_sub(start_column).max(1);
        let left = body[0] + metrics.padding_x + column as f32 * advance - pane.scroll[0];
        let top = body[1] + metrics.padding_y + row as f32 * metrics.line_height - pane.scroll[1];
        let host = [
            left,
            top,
            left + columns as f32 * advance,
            top + metrics.line_height,
        ];
        let (x, y) = (position.x as f32, position.y as f32);
        if x < host[0] || x > host[2] || y < host[1] || y > host[3] {
            return None;
        }
        Some(PreviewHexHover {
            surface,
            host,
            offset: from + token.range.start,
            text: token.text(line).to_owned(),
            rgba: token.rgba,
        })
    }

    /// Arm, move or retire the colour card's subject.
    ///
    /// No repaint of its own: the card is a *tip*, so what it costs is an entry
    /// in the anchor list the next frame builds, and the tip host's own clock
    /// decides whether anything is drawn. A pointer crossing a scheme file
    /// therefore costs exactly what a pointer crossing a tab strip costs.
    pub(crate) fn note_preview_hex_hover(&mut self, position: Option<PhysicalPosition<f64>>) {
        let over = position.and_then(|position| self.preview_hex_at(position));
        if over != self.window.preview_hex_hover {
            self.window.preview_hex_hover = over;
        }
    }

    /// A press on a block's scroll thumb takes hold of it.
    ///
    /// Returns whether the press was the thumb's. Asked before the edit surface
    /// claims the press, because the bar stands *over* the block it scrolls: a
    /// press there means the bar, the same way a press on a scrollbar in a text
    /// editor is never a press in the text.
    pub(crate) fn press_preview_block_thumb(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some((surface, index, bar)) = self.preview_block_bar_under(position) else {
            return Ok(false);
        };
        self.preview_block_drag = Some(PreviewBlockDrag {
            surface,
            index,
            grab: (position.x as f32 - bar.thumb[0]).clamp(0.0, bar.thumb[2] - bar.thumb[0]),
        });
        self.preview_block_hover = Some((surface, index));
        self.repaint_preview()?;
        Ok(true)
    }

    /// The pointer travelling with a thumb in hand.
    ///
    /// It keeps the pointer *outside* the block's own rectangle too, which is
    /// [`Self::drag_preview_selection`]'s rule and every scrollbar's: a drag
    /// that stopped tracking the moment it left a two-pixel band would be no
    /// better than the indicator it replaced.
    pub(crate) fn drag_preview_block_thumb(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(drag) = self.preview_block_drag else {
            return Ok(false);
        };
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // The gesture's own surface, not the one under the pointer: a hand that
        // has left the pane it took hold in is still holding that pane's thumb.
        // Asked of `preview_document_box` rather than of the tree, because the
        // surface may be the glance card — which is in no tree, and whose thumb
        // is dragged with the pointer well outside a 300-pixel frame.
        let Some(body) = self.preview_document_box(drag.surface, scale) else {
            return Ok(true);
        };
        let Some(pane) = self.preview_pane(drag.surface) else {
            return Ok(true);
        };
        let offset = pane.md_block_scroll.get(drag.index).copied().unwrap_or(0.0);
        let PreviewDocument::Markdown { blocks, layout, .. } = &pane.doc else {
            return Ok(true);
        };
        let found = preview_wide_blocks(
            body,
            seats::preview_markdown_metrics(scale),
            pane.scroll,
            (blocks, layout),
        )
        .find(|(index, _, _)| *index == drag.index)
        .and_then(|(_, clip, content)| {
            preview::scroll_bar(
                clip,
                preview::ScrollAxis::Horizontal,
                offset,
                content,
                scale,
            )
        });
        // The block stopped being wide — the pane grew, the document changed.
        // The gesture still owns the pointer until the button comes up; there is
        // simply nothing left for it to move.
        let Some(bar) = found else {
            return Ok(true);
        };
        let along = bar.along([position.x as f32, position.y as f32]);
        let wanted = preview::scroll_dragged_to(&bar, along, drag.grab);
        self.set_preview_block_scroll(drag.surface, drag.index, wanted)?;
        Ok(true)
    }

    /// Light the thumb under the pointer, or put the last one out.
    pub(crate) fn note_preview_block_hover(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
    ) -> Result<()> {
        let over = position
            .and_then(|position| self.preview_block_bar_under(position))
            .map(|(surface, index, _)| (surface, index));
        if over == self.preview_block_hover {
            return Ok(());
        }
        self.preview_block_hover = over;
        self.repaint_preview()
    }

    /// The stored offset, put back inside what the document actually has.
    ///
    /// Every write of `preview_scroll` goes through here — the wheel, the heal,
    /// and the reset a new file gets — so a body cannot end up parked past its
    /// own end on either axis. The two bodies with no scroller of their own (a
    /// table and a rendered markdown, which fit their pane or overflow it in
    /// ways slice 2 does not scroll) clamp to nothing, which is the truth.
    pub(crate) fn clamped_preview_scroll(
        &self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
        scroll: [f32; 2],
    ) -> [f32; 2] {
        let max = self.preview_max_scroll(surface, body, scale);
        [scroll[0].clamp(0.0, max[0]), scroll[1].clamp(0.0, max[1])]
    }

    /// How far each of the four bodies may be scrolled.
    ///
    /// **Every body answers for itself, in its own geometry.** Slice 2 left two
    /// of them answering "not at all" — a table and a rendered markdown clamped
    /// to nothing — and wrote the debt down as an open account: a `.csv` with a
    /// hundred rows and a `README.md` longer than the pane could be *drawn* past
    /// their own ends and never scrolled back. The account is settled here, and
    /// it is settled by asking each geometry for the extent it already computes
    /// rather than by teaching the mono path about two layouts that are not
    /// monospace grids.
    ///
    /// Neither of the two scrolls sideways, and that is not an omission: a table
    /// whose columns are derived from its own widest cells is as wide as it is,
    /// and markdown *wraps* to the pane, so a horizontal offset would be an
    /// offset into nothing.
    fn preview_max_scroll(&self, surface: PreviewSurface, body: [f32; 4], scale: f32) -> [f32; 2] {
        // **A glance card over a page scrolls its column, not a document** (user
        // ruling 2026-08-26). It is the one body in this window whose reach is
        // not a layout of text: a `.pdf` has no document on the card's pane at
        // all, and what it has instead is a page count and a slot per page. So
        // the answer comes from the card, which is where the count was turned
        // into a length — and everything downstream of this call, the wheel, the
        // thumb, the clamp, is the same code the source of an `.html` uses.
        if surface == PreviewSurface::Peek
            && let Some(column) = self.window.file_peek.as_ref().and_then(|peek| peek.column)
        {
            return [0.0, column];
        }
        let Some(pane) = self.preview_pane(surface) else {
            return [0.0, 0.0];
        };
        let (rows_height, columns) = self.preview_content_extent(surface, scale);
        preview_document_max_scroll(
            &pane.doc,
            body,
            scale,
            pane.mono_advance,
            rows_height,
            columns,
        )
    }

    /// Hand the file on the seat to whatever the system has registered for it.
    ///
    /// **The same one door out this window has always had** —
    /// [`Self::open_local_path`], and through it `bt_platform`'s own refusal for
    /// anything that is not a local file. What changed is where the door is: it
    /// used to be what a double click fell through to, and it is now a button
    /// the window offers *after* saying it cannot show the file itself.
    ///
    /// The acknowledgement is only printed when the system took it. A word that
    /// says "Opened" over a launch that was refused is the one thing worse than
    /// silence — the foot's own rule, applied to the card.
    pub(crate) fn open_preview_externally(&mut self) -> Result<()> {
        let Some(seat) = self.seats.preview() else {
            return Ok(());
        };
        self.open_preview_externally_on(self.preview_here(seat))
    }

    /// The same, on a surface the caller has already named (§7.39).
    ///
    /// A no-preview card in a **float** hands its file to the system exactly as
    /// the one in a pane does — the card is the same card, so its button is the
    /// same verb — and the only thing that differs is which surface names the
    /// file. Since the owner's ruling of 2026-09-12 the pane's own button comes
    /// through here too, because there is one verb and naming the surface is the
    /// only question it has.
    pub(crate) fn open_preview_externally_on(&mut self, surface: PreviewSurface) -> Result<()> {
        let Some(path) = self.preview_file_on(surface) else {
            return Ok(());
        };
        self.open_path_in_default_app(&path)
    }

    /// **The file this surface is showing, whichever lane it came down**
    /// (owner's ruling 2026-09-12).
    ///
    /// The system's handler is asked to open a *file*; there is no such door for
    /// a document this window composed out of a repository, which is what the
    /// `None` is.
    ///
    /// The picture is asked second and not instead: a surface showing one has no
    /// buffer at all (`preview_chrome_on`), so a card over a refused `.png` used
    /// to press a button that reached for a buffer that was never there and did
    /// nothing at all. The two lanes are one question here for
    /// [`Self::preview_chrome_on`]'s reason — a caller that asked only one of
    /// them would be answering for half the surfaces in this window.
    fn preview_file_on(&self, surface: PreviewSurface) -> Option<PathBuf> {
        self.preview_buffer_on(surface)
            .and_then(|buffer| buffer.source.file_path())
            .map(Path::to_path_buf)
            .or_else(|| {
                self.preview_picture(surface)
                    .map(|picture| picture.path.clone())
            })
    }

    /// What the card's one control says right now.
    ///
    /// Two words for 1300ms after a press and its own name the rest of the time
    /// — the same acknowledgement, and the same duration, the foot's "Revealed"
    /// uses (ruling 6, 2026-08-12: the four mock-up durations collapse to this
    /// one).
    pub(crate) fn preview_open_button_label(&self, now: Instant) -> &'static str {
        match self.window.preview_opened_at {
            Some(at) if now.saturating_duration_since(at) < FOOT_REVEAL_FEEDBACK => {
                preview_opened_label()
            }
            _ => preview_open_externally_label(),
        }
    }

    /// **Which surface's quick edit holds the keyboard** —
    /// `InputOwner::PreviewEdit`.
    ///
    /// Healed at the read, exactly as [`Self::files_keyboard_seat`] is and for
    /// the same reason: every way the answer can go stale (the surface closed,
    /// the document became a picture, the markdown flipped back to its render,
    /// the file turned out to be truncated and therefore read-only) is answered
    /// here rather than by remembering to write `None` at each of those places.
    /// The keyboard falls back to the shell the instant there is nothing to type
    /// into.
    ///
    /// The surface has to still *exist*, not merely still have a view: a float
    /// that closed and a leaf that was taken out of the tree both leave a pane
    /// behind until the next sweep, and a keyboard parked on one of them would be
    /// typing into a window nobody can see.
    pub(crate) fn preview_edit_focus(&self) -> Option<PreviewSurface> {
        let surface = self.preview_edit_focus?;
        (self.preview_surfaces().contains(&surface) && self.preview_is_editable(surface))
            .then_some(surface)
    }

    /// One key, with the quick edit holding the keyboard.
    ///
    /// Returns whether the key was the editor's at all. **Everything is**, once
    /// it has the focus (P139: "editor keys are the editor's — the terminal must
    /// not hear them"), which is why [`preview_edit::command`] is total and this
    /// returns `true` for every branch below it.
    ///
    /// **The save is not here** (ruling 9, 2026-08-12). It used to be, twice: in
    /// the editor's own vocabulary and again in a branch above this one that
    /// caught `Ctrl+S` when the seat was focused but the editor was not. Both are
    /// now the one scoped row of `shortcuts::BINDINGS`, resolved before this
    /// surface is asked — which is what "from any focus state" (mock-up
    /// 6139-6150) means when the condition is data rather than two call sites
    /// agreeing.
    pub(crate) fn preview_key(&mut self, event: &KeyEvent) -> Result<bool> {
        let Some(surface) = self.preview_edit_focus() else {
            return self.preview_browse_key(event);
        };
        let command = preview_edit::command(&event.logical_key, self.window.modifiers);
        // Repeats travel and type; they do not copy, paste or blur. Held Enter is
        // one continuous "again" and a held verb is not — the same line the files
        // tree draws between its travel keys and its verbs.
        if event.repeat
            && matches!(
                command,
                preview_edit::EditCommand::Copy
                    | preview_edit::EditCommand::Cut
                    | preview_edit::EditCommand::Paste
                    | preview_edit::EditCommand::SelectAll
                    | preview_edit::EditCommand::Release
            )
        {
            return Ok(true);
        }
        match command {
            preview_edit::EditCommand::Insert(text) => self.insert_into_preview(&text)?,
            preview_edit::EditCommand::Newline => {
                let eol = self
                    .preview_buffer_on(surface)
                    .and_then(|buffer| buffer.content.as_deref())
                    .map_or("\n", preview_edit::eol_of);
                self.insert_into_preview(eol)?;
            }
            // A literal tab. `tab-size: 4` is how one is *drawn* (mock-up 603);
            // expanding it on the way in would rewrite the indentation of every
            // file the preview was opened in.
            preview_edit::EditCommand::Tab => self.insert_into_preview("\t")?,
            preview_edit::EditCommand::Backspace => {
                self.edit_preview(|content, caret| preview_edit::backspace(content, caret))?;
            }
            preview_edit::EditCommand::Delete => {
                self.edit_preview(|content, caret| preview_edit::delete_forward(content, caret))?;
            }
            preview_edit::EditCommand::Move { motion, extend } => {
                self.move_preview_caret(motion, extend)?;
            }
            preview_edit::EditCommand::SelectAll => {
                let mut caret = self.preview_pane_mut(surface).caret;
                if let Some(content) = self
                    .preview_buffer_on(surface)
                    .and_then(|buffer| buffer.content.as_deref())
                {
                    preview_edit::select_all(content, &mut caret);
                }
                self.preview_pane_mut(surface).caret = caret;
                self.repaint_preview()?;
            }
            preview_edit::EditCommand::Copy => self.copy_preview_selection(),
            preview_edit::EditCommand::Cut => {
                self.copy_preview_selection();
                self.edit_preview(|content, caret| {
                    !caret.is_empty() && preview_edit::insert(content, caret, "")
                })?;
            }
            preview_edit::EditCommand::Paste => self.paste_into_preview()?,
            // Esc gives the keyboard back. Answered here rather than encoded,
            // which is exactly what §7.1.5's layering says: Esc reaches the child
            // only when the owner is the terminal.
            preview_edit::EditCommand::Release => {
                // **And on a rendered page it renders the block again** (T5 ③,
                // §7.1.3t). `Esc` is the plainest of the three gestures that
                // mean "I have finished with this" — see
                // [`Self::leave_preview_page`] for the other two and for what
                // leaving keeps.
                self.leave_preview_page(surface);
                self.repaint_preview()?;
            }
            preview_edit::EditCommand::Ignore => {}
        }
        Ok(true)
    }

    /// One key with the preview seat focused and **nothing to type into** —
    /// a picture, a table, a diff, a rendered markdown, a "no preview" card.
    ///
    /// **The keyboard has still left the shell** (ruling 2026-08-13, closing the
    /// gap the caret report opened). §7.1.5 lets characters into a PTY only while
    /// the owner is `Terminal`, and a focused preview is not one — so every key
    /// is answered here, and the ones that mean something answer by scrolling the
    /// document. It is the files column's `/* nothing to type into */` (mock-up
    /// 6199) applied to the other leaf that is not a shell, and it is what makes
    /// the three things the report asked to line up — the lit pane, the keyboard,
    /// and the caret — one fact instead of three.
    ///
    /// Travel keys honour repeats and nothing else does, which is the same line
    /// the tree draws: holding an arrow is one continuous "further".
    fn preview_browse_key(&mut self, event: &KeyEvent) -> Result<bool> {
        // **A field standing over this surface is asked before it** (§7.7 ②,
        // B69: 「search keys are the search box's」).
        //
        // This function ends in `_ => return Ok(true)`: a focused preview
        // swallows every key it does not use, which was written when the only
        // thing under it was a shell. The search capsule's second host is a
        // **page**, and a page's seat is a focused preview — so with the capsule
        // up on one, every character typed into it was being eaten here. Found
        // on the machine, 2026-08-22: `Ctrl+F` over a page raised a capsule
        // nothing could be typed into.
        //
        // The condition is the caret's, not the capsule's: B81's second stance —
        // search open, hands back on the surface — is exactly the state in which
        // this surface should go on swallowing.
        if self.window.search.is_focused() {
            return Ok(false);
        }
        let Some(surface) = self.preview_keyboard_surface() else {
            return Ok(false);
        };
        // **A player's five keys, before every reading below** (user ruling
        // 2026-08-28; §7.44 ②).
        //
        // Space, `←`/`→`, `↑`/`↓` and `M`, and they are asked *first* because
        // every one of them means something else to the surface underneath: the
        // arrows scroll a document, Space pages one. A surface that is playing a
        // recording is a surface whose arrows are a playhead — which is what the
        // shell page's own `keydown` said by being inside the page, and is now
        // said by being the first rung.
        //
        // **On whichever surface holds the keyboard**, which is the whole of
        // §7.34 reaching this ruling: `preview_keyboard_surface` has already
        // decided between a float and a docked pane, so a player in a window
        // answers its keys exactly when that window has them and a docked one
        // when the pane does. The glance card is never that surface and never
        // takes a key — it is read-only by its own founding ruling, and a card
        // that swallowed Space would be a hover eating the shell's keys.
        if event.state.is_pressed()
            && !self.window.modifiers.control_key()
            && !self.window.modifiers.alt_key()
            && !self.window.modifiers.super_key()
        {
            let now = Instant::now();
            let modified = false;
            if let Some(seat) = self.window.video.get_mut(surface)
                && seat.key_press(&event.logical_key, modified, now)
            {
                self.refresh_chrome();
                self.present_chrome_change()?;
                return Ok(true);
            }
        }
        // **A graph is a list, not a document** (V14). The surface that holds it
        // is an ordinary preview surface and got the keyboard the ordinary way —
        // a press into it focused it — but what its arrows mean is "the row
        // above" and not "twenty pixels up", so it is asked before the scroll
        // below.
        //
        // **Asked of a window as well as a pane** (user report, 2026-08-20).
        // This used to narrow to `PreviewSurface::Seat` before asking either
        // question, which was the keyboard half of the same defect the paint
        // had: a graph torn off into a window answered its arrows by scrolling
        // an empty document twenty pixels at a time.
        //
        // It is asked and not *told*: a key it does not claim (`Esc` with nothing
        // to fold) falls through to the scroll, which swallows it as every
        // focused preview does. The one key that must not be eaten here is the
        // one this page has no use for, and saying so is the graph's own answer
        // rather than a condition written twice.
        // **The search field is asked first, and only while it holds the
        // keyboard** (T4). It takes *every* key it is given — a field that let an
        // unrecognised character fall through to the list under it would be a
        // field you could type `j` into and watch the graph scroll.
        if self.graph_search_focused(surface) && self.graph_search_key(surface, event)? {
            return Ok(true);
        }
        // **A rendered page's three verbs** — `Ctrl+C`, `Ctrl+A` and `Esc`
        // (user report 2026-08-28). Asked before the scroll below for the
        // graph's reason: what those keys mean on this surface is about the
        // *words*, not about twenty pixels — and `Esc` with nothing selected
        // falls back through to whatever else wants it, exactly as the graph's
        // does.
        if self.preview_text_key(surface, event)? {
            return Ok(true);
        }
        if self.window.git_graphs_shown.contains_key(&surface)
            && let Some(key) = graph_key_of(&event.logical_key, self.window.modifiers)
            && self.graph_key(surface, key)?
        {
            return Ok(true);
        }
        // **A picture answers the keyboard with its own four verbs** (ticket
        // #60), and answers before the scroll below for the graph's reason: what
        // the keys mean on this surface is "larger" and "smaller", not "twenty
        // pixels down", and a picture has nothing to scroll anyway. A key it does
        // not claim falls through and is swallowed by the scroll exactly as every
        // other key on a focused preview is.
        if let Some((body, image_px)) = self.preview_image_geometry(surface)
            && let Some(zoomed) = image_zoom_key(
                self.preview_image_zoom(surface),
                &event.logical_key,
                body,
                image_px,
            )
        {
            self.set_preview_image_zoom(surface, zoomed)?;
            return Ok(true);
        }
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(body) = self.preview_surface_body_rect(surface, scale) else {
            // A seat with no body to scroll still owns the key: there is no
            // terminal under it either way.
            return Ok(true);
        };
        let page = (body[3] - body[1]).max(1.0);
        let line = seats::preview_text_metrics(scale).line_height;
        let step = match &event.logical_key {
            Key::Named(NamedKey::ArrowDown) => [0.0, line],
            Key::Named(NamedKey::ArrowUp) => [0.0, -line],
            Key::Named(NamedKey::ArrowRight) => [line, 0.0],
            Key::Named(NamedKey::ArrowLeft) => [-line, 0.0],
            Key::Named(NamedKey::PageDown) | Key::Named(NamedKey::Space) => [0.0, page],
            Key::Named(NamedKey::PageUp) => [0.0, -page],
            // Home and End are the document's ends, expressed as an offset the
            // clamp will cut to size — one door for every write of the stored
            // scroll, which is `clamped_preview_scroll`'s whole rule.
            Key::Named(NamedKey::Home) => [0.0, f32::MIN],
            Key::Named(NamedKey::End) => [0.0, f32::MAX],
            // **A key that would have inserted, on a page that will not take
            // one** (owner's ruling 2026-09-12). This surface has no caret —
            // that is what put the key here rather than in
            // [`Self::preview_key`] — so a letter, a `Tab`, an `Enter` or a
            // `Backspace` arriving on it is a reader trying to type into
            // something this window is refusing to edit, and the reason floats
            // for two seconds.
            //
            // **Asked after the reading keys and not before them.** `Space` and
            // the arrows would insert on a page with a caret and page a document
            // without one, and the one they mean here is the reading: a reader
            // who cannot edit a file is still reading it, and a pill raised by
            // the page-down key would be this window answering a question nobody
            // asked. So the check is the last arm rather than the first, and
            // everything the reading claimed keeps it.
            key => {
                if matches!(
                    preview_edit::command(key, self.window.modifiers),
                    preview_edit::EditCommand::Insert(_)
                        | preview_edit::EditCommand::Newline
                        | preview_edit::EditCommand::Tab
                        | preview_edit::EditCommand::Backspace
                        | preview_edit::EditCommand::Delete
                ) {
                    self.refuse_preview_edit(surface);
                }
                return Ok(true);
            }
        };
        let scroll = self.preview_pane_mut(surface).scroll;
        let wanted = [scroll[0] + step[0], scroll[1] + step[1]];
        let scrolled = self.clamped_preview_scroll(surface, body, scale, wanted);
        if scrolled == scroll {
            return Ok(true);
        }
        self.preview_pane_mut(surface).scroll = scrolled;
        self.repaint_preview()?;
        Ok(true)
    }

    /// A composition landing in the preview (user report, 2026-08-12).
    ///
    /// The commit goes through [`Self::insert_into_preview`] — the *same* door a
    /// typed character uses — so the dirty bit, the caret reveal, the notice and
    /// the read-only refusal all answer exactly as they do for the keyboard.
    /// One insert path per surface, not one per input method.
    pub(crate) fn preview_ime(&mut self, event: Ime) -> Result<()> {
        match event {
            Ime::Preedit(text, cursor_range) => {
                // A collapsed range is the caret; an open one is the IME's
                // target clause and not a caret at all — see
                // [`preedit_caret_byte`].
                self.window.preedit = (!text.is_empty()).then_some(Preedit {
                    text,
                    cursor_byte: preedit_caret_byte(cursor_range),
                });
                self.repaint_preview()
            }
            Ime::Commit(text) => {
                self.window.preedit = None;
                self.insert_into_preview(&text)
            }
            // Reached only for `Preedit`/`Commit`; the window's own bookkeeping
            // is nobody's surface and stays in `ime_input`.
            Ime::Enabled | Ime::Disabled => Ok(()),
        }
    }

    /// Where the IME should hang its candidate list while the preview is being
    /// typed into — the edit caret's box, in window pixels.
    ///
    /// Win32's caret is thread-level and IMM32 asks the focused window, so there
    /// is nothing to switch but the rectangle: the same two calls
    /// ([`Self::apply_ime_cursor_area`]) serve whichever surface is composing.
    ///
    /// **The surface is the one the letters are going to, and it is asked for
    /// through the same door** (M1-8; `docs/DESIGN.md` §13.16 ②). This used to
    /// start at [`Self::preview_edit_focus`] while the rung above it was decided
    /// by [`Self::preview_keyboard_surface`] — which is also what
    /// [`Self::insert_into_preview`] inserts through — so a preview *seat*
    /// holding the keyboard without the quick edit's focus was answered
    /// `ImeOwner::Preview` by [`ime_owner`], had its commits inserted, and
    /// published no caret at all: two doors, one composition, and the candidate
    /// list left standing wherever it was last put. That is §7.1.5a″'s own rule
    /// one surface along — the answer that places the list has to be the answer
    /// that routed the letters — and it is one door here for the same reason it
    /// is one door there.
    pub(crate) fn preview_ime_cursor_area(&self) -> Option<ImeCursorArea> {
        let surface = self.preview_keyboard_surface()?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let body = self.preview_surface_body_rect(surface, scale)?;
        // **On a rendered page the box comes from the source block** (T5 ②,
        // §7.1.3t), for [`Self::reveal_live_markdown_caret`]'s reason: the
        // geometry below is the whole body's monospace grid, and this page is
        // one monospace block standing between paragraphs of prose. A caret in a
        // gap has no block, and then this turn simply has nothing to say —
        // [`Self::offer_ime_caret`] leaves the candidate list where it was
        // rather than moving it somewhere invented.
        if self.preview_shows_live_markdown(surface) {
            return self.live_markdown_ime_cursor_area(surface, body, scale);
        }
        let (line, column) = self.preview_caret_position(surface)?;
        let advance = self.preview_pane(surface)?.mono_advance;
        let (rows_height, columns) = self.preview_content_extent(surface, scale);
        let geometry =
            self.preview_doc_text_geometry(surface, body, scale, rows_height, columns, advance);
        let (row, column) = self
            .preview_wrap(surface)
            .map_or((line, column), |wrap| preview_caret_row(wrap, line, column));
        let box_of_row = geometry.line_rect(row);
        // Clamped into the body, for `ime_cursor_area_for_metrics`'s reason: a
        // caret scrolled out of sight would put the candidate list somewhere the
        // user is not looking, or off the window entirely.
        let x = (box_of_row[0] + advance * column as f32).clamp(body[0], body[2]);
        let y = box_of_row[1].clamp(body[1], body[3]);
        Some(ImeCursorArea {
            x: x.round() as i32,
            y: y.round() as i32,
            width: advance.round().max(1.0) as u32,
            height: (box_of_row[3] - box_of_row[1]).round().max(1.0) as u32,
        })
    }

    /// **Where the composition hangs while a rendered Markdown page is being
    /// typed into** (T5 ②, §7.1.3t) — the source block's own row and column,
    /// through the very fold the painter drew them at.
    ///
    /// Clamped into the body for [`Self::preview_ime_cursor_area`]'s reason: a
    /// caret scrolled out of sight would put the candidate list somewhere the
    /// reader is not looking, or off the window entirely.
    fn live_markdown_ime_cursor_area(
        &self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
    ) -> Option<ImeCursorArea> {
        let caret = self.preview_live_caret(surface)?;
        // **On a prose block it is the bar's own rectangle** (§7.1.3w), which is
        // this function's own rule one face over: a candidate list placed from a
        // second derivation is a list standing beside the caret it claims to
        // follow. There is no cell to be a character wide on a proportional
        // face, so the strip the list may not cover is the caret's own bar.
        if let Some(prose) = self
            .preview_pane(surface)
            .and_then(|pane| pane.md_prose.as_ref())
        {
            // **The box hangs at the composition's own caret once there is a
            // composition** (§7.1.3q), and not at the byte it was typed in front
            // of: a list offering to finish `nikan` that stood where the `n`
            // went in would sit over the letters it is offering to replace.
            let [x, top, _, bottom] = prose
                .composition
                .as_ref()
                .and_then(|composition| composition.caret)
                .or_else(|| prose.caret(caret.caret))?;
            let width = (bt_render::CURSOR_BAR_WIDTH_LOGICAL_PX * scale)
                .round()
                .max(1.0);
            return Some(ImeCursorArea {
                x: x.clamp(body[0], body[2]).round() as i32,
                y: top.clamp(body[1], body[3]).round() as i32,
                width: width as u32,
                height: (bottom - top).round().max(1.0) as u32,
            });
        }
        let (box_of_block, source) = self.markdown_source_box(surface, scale)?;
        let wrap = source.wrap((box_of_block[2] - box_of_block[0]).max(1.0));
        let (row, column) = preview_live::BlockRows {
            text: &source.text,
            start: source.range.start,
            wrap: &wrap,
        }
        .row_of(caret.caret)?;
        // **At the composition's own caret once there is a composition**, which
        // is the prose face's rule one face over and the same reason: a list
        // offering to finish `nikan` that stood where the `n` went in would sit
        // over the letters it is offering to replace.
        let column = column
            + self.preview_preedit(surface).map_or(0, |preedit| {
                preview_edit::column_of(
                    &preedit.text,
                    preedit.cursor_byte.unwrap_or(preedit.text.len()),
                )
            });
        // The painter's own cell ([`markdown_source_cell`]): a candidate list
        // placed from a second derivation is a list standing beside the caret it
        // claims to follow, and on a line of Chinese the two derivations used to
        // differ by a character's width every character.
        let cell = markdown_source_cell(source, box_of_block, row, column);
        let x = cell[0].clamp(body[0], body[2]);
        let y = cell[1].clamp(body[1], body[3]);
        Some(ImeCursorArea {
            x: x.round() as i32,
            y: y.round() as i32,
            width: (cell[2] - cell[0]).round().max(1.0) as u32,
            height: (cell[3] - cell[1]).round().max(1.0) as u32,
        })
    }

    /// Type into the buffer the keyboard is in.
    fn insert_into_preview(&mut self, text: &str) -> Result<()> {
        self.edit_preview(|content, caret| preview_edit::insert(content, caret, text))
    }

    /// Run one edit against the buffer the keyboard's surface is showing.
    ///
    /// **The pool's buffer, not a copy of it.** A file open in two panes is one
    /// buffer (§7.1.3), so an edit goes to the pool or it forks — and the caret
    /// travels beside it because a caret is the *view's* (ruling 8⑧), which is
    /// why the two live in different places and move together here.
    fn edit_preview(
        &mut self,
        edit: impl FnOnce(&mut preview_text::EditText<'_>, &mut preview_edit::EditCaret) -> bool,
    ) -> Result<()> {
        let Some(surface) = self.preview_keyboard_surface() else {
            return Ok(());
        };
        let mut caret = self.preview_pane_mut(surface).caret;
        // Asked of the surface, because the render has nothing to type into and
        // the source of the same file has.
        if !self.preview_is_editable(surface) {
            return Ok(());
        }
        let Some(buffer) = self.preview_buffer_on_mut(surface) else {
            return Ok(());
        };
        // Through the caret's own door, because this is the one caller that has
        // a caret to file: the undo log lives on the buffer and the caret it
        // remembers is the one that made the change (T3, `preview_undo`).
        let changed = buffer.edit_by_caret(&mut caret, |content, caret| edit(content, caret));
        let pane = self.preview_pane_mut(surface);
        pane.caret = caret;
        if changed {
            // A keystroke answers whatever the last save had to say: a conflict
            // notice still standing over a body that has moved on is a sentence
            // about a state that no longer exists.
            pane.notice = None;
        }
        self.reveal_preview_caret(surface);
        self.repaint_preview()
    }

    /// Move the caret, and bring it back into view.
    fn move_preview_caret(&mut self, motion: preview_edit::Motion, extend: bool) -> Result<()> {
        let Some(surface) = self.preview_keyboard_surface() else {
            return Ok(());
        };
        let Some(buffer) = self.preview_buffer_on(surface) else {
            return Ok(());
        };
        let Some(content) = buffer.content.as_deref() else {
            return Ok(());
        };
        let rows = self.preview_page_rows(surface);
        let mut caret = self.preview_pane(surface).expect("preview pane").caret;
        // **Up and Down walk visual rows; Home and End walk the logical line.**
        // The `<textarea>` convention, and the one a reader expects: on a
        // wrapped line, Down that jumped the whole paragraph would skip most of
        // what is on the screen, while a Home that stopped at the start of the
        // *row* would leave no key that reaches the start of the line at all.
        //
        // **And on a rendered page they walk the source block's rows, then the
        // file's lines** (T5 ②, §7.1.3t). The block under the caret is the only
        // part of that page with rows at all; step off its top or its bottom and
        // the step becomes a step of the file's lines, which lands in the
        // neighbouring block or in the gap between them — and the next parse
        // makes whichever it is the source block.
        let stepped = if self.preview_shows_live_markdown(surface) {
            let scale = self.window.renderer.metrics().scale_factor as f32;
            let prose = self
                .preview_pane(surface)
                .and_then(|pane| pane.md_prose.as_ref());
            let moved = match self.markdown_source_box(surface, scale) {
                Some((box_of_block, source)) => {
                    let wrap = source.wrap((box_of_block[2] - box_of_block[0]).max(1.0));
                    preview_live::step_by_row(
                        content,
                        Some(preview_live::CaretRows::Mono(preview_live::BlockRows {
                            text: &source.text,
                            start: source.range.start,
                            wrap: &wrap,
                        })),
                        &mut caret,
                        motion,
                        rows,
                    )
                }
                // **A prose block walks the shaper's rows** (§7.1.3w): the rows
                // the reader can see, which on this face are a soft wrap of the
                // block's own lines and not a fold of a grid. A caret in a gap
                // has no block to walk either way: the gap's empty line is one
                // place, and the way out of it is the file's own lines.
                None => preview_live::step_by_row(
                    content,
                    prose.map(preview_live::CaretRows::Prose),
                    &mut caret,
                    motion,
                    rows,
                ),
            };
            moved.then_some(())
        } else {
            self.preview_wrap(surface)
                .filter(|wrap| wrap.wraps())
                .and_then(|wrap| step_preview_caret_by_row(content, &mut caret, motion, wrap, rows))
        };
        if stepped.is_none() {
            preview_edit::move_caret_indexed(
                content,
                buffer.line_starts(),
                &mut caret,
                motion,
                extend,
                rows,
            );
        } else if !extend {
            caret.anchor = caret.caret;
        }
        self.preview_pane_mut(surface).caret = caret;
        self.reveal_preview_caret(surface);
        self.repaint_preview()
    }

    /// **Somebody has just tried to change a page that will not take it** —
    /// float the reason for two seconds (owner's ruling 2026-09-12).
    ///
    /// The two gestures are the two that mean it, and they are the two the
    /// refusal is invisible at: a press that would have seated a caret
    /// ([`Self::press_preview_body`], where the editability gate turns it away)
    /// and a key that would have inserted ([`Self::preview_browse_key`], where
    /// one lands on a surface with no caret to put it in). Before this ruling
    /// both were simply nothing happening, with the reason standing in a band at
    /// the bottom of the pane that the reader had no cause to be looking at.
    ///
    /// **Every further attempt restarts the clock** rather than being ignored or
    /// stacking a second pill: a reader typing a word into a read-only file is
    /// making one gesture, not six, and the answer should still be on the glass
    /// when they stop.
    ///
    /// Raised only where there is a reason to give — a picture, a diff and a
    /// commit graph are not *refusing* anything, they simply have no text in
    /// them, and a pill that said so on every keystroke would be this window
    /// explaining what a `.png` is.
    fn refuse_preview_edit(&mut self, surface: PreviewSurface) -> bool {
        let now = Instant::now();
        if self.preview_standing_fact(surface, now).is_none() {
            return false;
        }
        self.window.preview_refusal = Some((surface, now));
        let _ = self.refresh_overlay();
        let _ = self.present_chrome_change();
        true
    }

    /// The reason this surface is floating, while it still is.
    fn preview_refusal_reason(&self, surface: PreviewSurface, now: Instant) -> Option<&str> {
        let (refused, at) = self.window.preview_refusal?;
        (refused == surface && now.saturating_duration_since(at) < PREVIEW_REFUSAL_HOLD)
            .then(|| self.preview_standing_fact(surface, now))
            .flatten()
    }

    /// When the refusal now on the glass is due to go away, so the loop can be
    /// up for it — the acknowledgements' own wake-up, one surface along.
    pub(crate) fn preview_refusal_deadline(&self) -> Option<Instant> {
        let (_, at) = self.window.preview_refusal?;
        Some(at + PREVIEW_REFUSAL_HOLD)
    }

    /// **What this surface's news pill is saying this frame**, and what it
    /// offers (owner's ruling 2026-09-12).
    ///
    /// Three kinds of news and one slot, in the order a reader can act on them:
    ///
    /// 1. **the reason an edit was refused**, because it is an answer to a
    ///    gesture made a heartbeat ago and the reader is waiting for it;
    /// 2. **a decision they owe** — a file rewritten or deleted under unsaved
    ///    edits — which stays until it is answered (the 2026-08-15 ruling,
    ///    unchanged) and is therefore what a pill is showing most of the time it
    ///    is up at all;
    /// 3. **a confirmation**, which expires by itself.
    ///
    /// The standing facts are not here and that is the ruling's other half: a
    /// read-only body and a `.gif` that will not move are *states*, they are
    /// worn as the padlock in the row above, and a sentence that stood on the
    /// glass for as long as the file was open would be the band this ruling
    /// retired wearing a new shape.
    pub(crate) fn preview_pill_say(
        &self,
        surface: PreviewSurface,
        now: Instant,
    ) -> Option<(String, &'static [notice::NoticeVerb])> {
        if let Some(reason) = self.preview_refusal_reason(surface, now) {
            // **One way out, and it is the one the refused page already offers**
            // (§7.7 ④″, `3af60fa`): this window will not edit the file, and the
            // machine has something that will.
            return Some((
                reason.to_owned(),
                &[notice::NoticeVerb::OpenExternally] as &'static [notice::NoticeVerb],
            ));
        }
        if let Some(state) = self.preview_disk_notice_on(surface) {
            return Some((state.text().to_owned(), state.verbs()));
        }
        // Which key the reveal was written under is the host's, exactly as it is
        // where the two surfaces dress themselves: a pane's confirmation belongs
        // to its seat and a window's to the window.
        let revealed = match surface {
            PreviewSurface::Seat(leaf) => RevealedFoot::Preview(leaf.seat),
            PreviewSurface::Float(id) => RevealedFoot::Float(id),
            PreviewSurface::Peek => return None,
        };
        let flashed = if self.foot_reveal_is_fresh(revealed, now) {
            Some(foot_revealed_label().to_owned())
        } else {
            // **The confirmation and not the refusal.** A save that was turned
            // away does not expire — it is a standing fact and wears the
            // padlock — so the one thing this slot takes from the save ledger is
            // the word that *is* news.
            self.preview_save_notice(surface, now)
                .filter(|notice| *notice == preview::preview_saved_notice())
                .map(str::to_owned)
        };
        flashed.map(|word| (word, &[] as &'static [notice::NoticeVerb]))
    }

    /// **The standing fact this surface's foot hangs on its right hand**, if it
    /// is owed one (user ruling, 2026-08-15).
    ///
    /// Standing facts only, and the word is doing work: what belongs here is
    /// true of the buffer for as long as you are looking at it, which is what
    /// earns a permanent corner of the strip. **Being read-only is one**, in all
    /// three of the ways a body can be
    /// ([`preview::PreviewBuffer::read_only_notice`]): the head of a file nobody
    /// has asked to edit yet, a body some of whose bytes would not read, and a
    /// file past the editing ceiling. A save's *refusal* is the other, and it
    /// qualifies for the same reason the wording says it does: it does not
    /// expire, because a warning that fades is a warning the user is entitled to
    /// have missed.
    ///
    /// A successful save is **not** one. It is news, it expires, and it has its
    /// own place at the strip's left where the reveal's confirmation goes —
    /// which is also why it is filtered out here rather than ranked below the
    /// others: the two halves of the strip answer different questions.
    ///
    /// The two can never both be owed: a read-only buffer has no save to
    /// report.
    pub(crate) fn preview_standing_fact(
        &self,
        surface: PreviewSurface,
        now: Instant,
    ) -> Option<&str> {
        self.animation_refusal_notice(surface)
            .or_else(|| {
                self.preview_buffer_on(surface)
                    .and_then(preview::PreviewBuffer::read_only_notice)
            })
            .or_else(|| {
                self.preview_save_notice(surface, now)
                    .filter(|notice| *notice != preview::preview_saved_notice())
            })
    }

    /// **Why the picture on this surface is standing still**, when it is a
    /// `.gif` this window would not play (user report 2026-09-10).
    ///
    /// A standing fact, which is what earns a place on this strip: it is true of
    /// the file for as long as you are looking at it, and it does not expire.
    ///
    /// It is first among the three because it is the only one of them about what
    /// is *on the glass*: the other two are about a text body, and a surface
    /// showing a picture has no body to be read-only about. Two of the four
    /// refusals say nothing at all — see
    /// [`animation::AnimationRefusal::is_worth_saying`] — because a `.gif` that
    /// is one still picture looks exactly like a still picture, which is what it
    /// is.
    fn animation_refusal_notice(&self, surface: PreviewSurface) -> Option<&'static str> {
        let path = self.animation_path_of(surface)?;
        let key = normalized_local_image_path_key(&path);
        let AnimationEntry::Refused(refusal) = self.window.animations.get(&key)? else {
            return None;
        };
        animation_refusal_notice(*refusal)
    }

    /// How many lines this surface's edit surface can show — what a page is.
    fn preview_page_rows(&self, surface: PreviewSurface) -> usize {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(body) = self.preview_surface_body_rect(surface, scale) else {
            return 1;
        };
        let metrics = seats::preview_text_metrics(scale);
        (((body[3] - body[1]) / metrics.line_height).floor() as usize).max(1)
    }

    /// Scroll until the caret is on screen.
    ///
    /// **The minimum move, on each axis independently.** A caret that walked off
    /// the bottom brings the body up by exactly one line rather than recentring,
    /// which is what every text field does and the only behaviour that makes
    /// holding Down look like reading rather than like jumping.
    fn reveal_preview_caret(&mut self, surface: PreviewSurface) {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(body) = self.preview_surface_body_rect(surface, scale) else {
            return;
        };
        // **A rendered page counts in blocks, not in lines** (T5, §7.1.3t).
        // Every number below is the source face's — a row is a line height from
        // the top of the body — and on a page of headings, tables and pictures
        // the caret's line number says nothing about where on the glass it is.
        // The block's own box does, and that is what the other reading uses.
        if self.preview_shows_live_markdown(surface) {
            self.prepare_markdown_caret_view(surface, body, scale);
            self.reveal_live_markdown_caret(surface, body, scale);
            self.ensure_markdown_viewport(surface, body, scale);
            return;
        }
        let Some((line, column)) = self.preview_caret_position(surface) else {
            return;
        };
        let Some(pane) = self.preview_pane(surface) else {
            return;
        };
        let metrics = seats::preview_text_metrics(scale);
        let advance = pane.mono_advance;
        // The *drawn* row and the column inside it: on a wrapped surface the
        // caret's line number is not where it is on the glass, and scrolling to
        // a line number would put a caret on the fortieth row of a line off the
        // bottom of a pane that thinks it is showing the first.
        let (row, column) = self.preview_wrap(surface).map_or((line, column), |wrap| {
            let (row, start) = wrap.row_of(line, column);
            (row, column - start)
        });
        let top = metrics.padding_y + metrics.line_height * row as f32;
        let bottom = top + metrics.line_height;
        let left = metrics.padding_x + advance * column as f32;
        let mut scroll = pane.scroll;
        let height = body[3] - body[1];
        let width = body[2] - body[0];
        scroll[1] = scroll[1].min(top).max(bottom - height);
        // The caret's own cell has to fit, not just its left edge — otherwise the
        // character being typed is the one thing off the right of the pane.
        scroll[0] = scroll[0]
            .min(left - metrics.padding_x)
            .max(left + advance + metrics.padding_x - width);
        let scrolled = self.clamped_preview_scroll(surface, body, scale, scroll);
        self.preview_pane_mut(surface).scroll = scrolled;
    }

    /// **Bring the caret's row of the source block into view** — the rendered
    /// page's half of [`Self::reveal_preview_caret`] (T5, §7.1.3t).
    ///
    /// Vertically only, because the page has no horizontal axis: every block is
    /// laid out inside the measure column and a source block folds rather than
    /// running off the side (§7.1.3q), so there is never a column out of reach
    /// to scroll to.
    ///
    /// A caret in a gap moves nothing at all, and that is the gap rule saying
    /// so: its empty line is drawn into the margin the page had already
    /// collapsed between two blocks, so it is on the glass exactly when the
    /// block above it is.
    fn reveal_live_markdown_caret(&mut self, surface: PreviewSurface, body: [f32; 4], scale: f32) {
        let Some(caret) = self.preview_live_caret(surface) else {
            return;
        };
        let scroll = self
            .preview_pane(surface)
            .map_or([0.0, 0.0], |pane| pane.scroll);
        // **A prose block's rows are the shaper's** (§7.1.3w), and the arithmetic
        // under them is this one's: back out of window pixels into the content
        // the scroll is measured in.
        if let Some(prose) = self
            .preview_pane(surface)
            .and_then(|pane| pane.md_prose.as_ref())
        {
            let seat = prose.row_of(caret.caret).and_then(|(row, _)| {
                let row = prose.rows.get(row)?;
                let top = row.top - body[1] + scroll[1];
                Some((top, top + row.height))
            });
            let Some((top, bottom)) = seat else {
                return;
            };
            let mut wanted = scroll;
            wanted[1] = wanted[1].min(top).max(bottom - (body[3] - body[1]));
            let scrolled = self.clamped_preview_scroll(surface, body, scale, wanted);
            self.preview_pane_mut(surface).scroll = scrolled;
            return;
        }
        // The block's borrow ends with this expression, before the scroll is
        // written back through the same runtime.
        let seat = {
            let Some((box_of_block, source)) = self.markdown_source_box(surface, scale) else {
                return;
            };
            let wrap = source.wrap((box_of_block[2] - box_of_block[0]).max(1.0));
            let rows = preview_live::BlockRows {
                text: &source.text,
                start: source.range.start,
                wrap: &wrap,
            };
            rows.row_of(caret.caret).map(|(row, _)| {
                // Back out of window pixels into the content the scroll is
                // measured in, so the number written below is the same kind of
                // number the one being replaced was.
                let block_top = box_of_block[1] - body[1] + scroll[1];
                let top = block_top + source.line_height * row as f32;
                (top, top + source.line_height)
            })
        };
        let Some((top, bottom)) = seat else {
            return;
        };
        let mut wanted = scroll;
        wanted[1] = wanted[1].min(top).max(bottom - (body[3] - body[1]));
        let scrolled = self.clamped_preview_scroll(surface, body, scale, wanted);
        self.preview_pane_mut(surface).scroll = scrolled;
    }

    /// Which row and column this surface's caret stands on.
    fn preview_caret_position(&self, surface: PreviewSurface) -> Option<(usize, usize)> {
        let content = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.as_deref())?;
        let caret = self.preview_pane(surface)?.caret;
        let starts = self.preview_buffer_on(surface)?.line_starts();
        let caret = preview_edit::normalize(content, caret.caret);
        let line = preview_edit::line_index(starts, caret);
        let (start, _) = preview_edit::line_bounds(content, starts, line);
        let text = preview_edit::line_text(content, starts, line);
        Some((line, preview_edit::column_of(text, caret - start)))
    }

    /// Put the selection on the clipboard, through the door the terminal's own
    /// copy already uses.
    pub(crate) fn copy_preview_selection(&mut self) {
        let Some(surface) = self.preview_keyboard_surface() else {
            return;
        };
        let Some(caret) = self.preview_pane(surface).map(|pane| pane.caret) else {
            return;
        };
        let Some(text) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.as_deref())
            .map(|content| caret.selected(content).to_owned())
            .filter(|text| !text.is_empty())
        else {
            return;
        };
        if let Err(error) = write_terminal_clipboard_text(&text) {
            eprintln!("recoverable preview copy failure: {error:#}");
        }
    }

    /// Paste into the buffer, with the breaks the *file* uses.
    ///
    /// A clipboard on this platform carries CRLF whatever it was copied from, so
    /// pasting it verbatim into a file written with bare newlines is how a
    /// one-line paste turns the next diff into a whole-file rewrite.
    pub(crate) fn paste_into_preview(&mut self) -> Result<()> {
        let text = match hang_watch::during(hang_watch::Station::ClipboardRead, || {
            bt_platform::clipboard_text()
        }) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("recoverable preview paste failure: {error}");
                return Ok(());
            }
        };
        if text.is_empty() {
            return Ok(());
        }
        let eol = self
            .preview_keyboard_surface()
            .and_then(|surface| self.preview_buffer_on(surface))
            .and_then(|buffer| buffer.content.as_deref())
            .map_or("\n", preview_edit::eol_of);
        let text = preview_edit::with_eol(&text, eol);
        self.insert_into_preview(&text)
    }

    /// Write the buffer the keyboard is in back to its file (mock-up 6139-6150).
    ///
    /// Nothing happens to a clean buffer — the mock-up's own guard, and the
    /// reason is the acknowledgement: "Saved" printed over a save that had
    /// nothing to write teaches the word to mean "the key worked" rather than
    /// "the file changed".
    pub(crate) fn save_preview(&mut self) -> Result<()> {
        let Some(surface) = self.preview_keyboard_surface() else {
            return Ok(());
        };
        self.save_preview_on(surface)
    }

    /// **Walk the buffer's history**, one entry either way (ticket T3).
    ///
    /// Reached exactly as [`Self::save_preview`] is, and for the same reason:
    /// `Ctrl+Z` means "wherever the keyboard is", so the surface is
    /// [`Self::preview_keyboard_surface`] — the edit focus if there is one, else
    /// the focused float, else the focused seat's pane. A surface with nothing to
    /// type into answers `preview_is_editable` with `false` and this does
    /// nothing, which is the same silence a rendered page keeps for every other
    /// key it swallows.
    ///
    /// **The log is the buffer's and the caret is the pane's**, so an undo
    /// pressed in either of two panes on one file takes back that file's last
    /// change whichever pane made it, and hands *this* pane the caret that made
    /// it. The other pane's caret is not touched: it is clamped into the body the
    /// next time it is used — every edit and every motion begins with
    /// [`preview_edit::EditCaret::heal`], and the caret the glass draws goes
    /// through `normalize` — and otherwise left where its own reader left it.
    /// That is already what happens when the other pane types, and an undo is a
    /// change to the body like any other.
    ///
    /// The notice comes down for `edit_preview`'s reason: a conflict still
    /// standing over a body that has moved on is a sentence about a state that no
    /// longer exists.
    pub(crate) fn step_preview_history(&mut self, step: Step) -> Result<()> {
        let Some(surface) = self.preview_keyboard_surface() else {
            return Ok(());
        };
        if !self.preview_is_editable(surface) {
            return Ok(());
        }
        let Some(buffer) = self.preview_buffer_on_mut(surface) else {
            return Ok(());
        };
        let moved = match step {
            Step::Back => buffer.undo_edit(),
            Step::Forward => buffer.redo_edit(),
        };
        // Nothing left in that direction is a press with nothing to say.
        let Some(caret) = moved else {
            return Ok(());
        };
        let pane = self.preview_pane_mut(surface);
        pane.caret = caret;
        pane.notice = None;
        self.reveal_preview_caret(surface);
        self.repaint_preview()
    }

    /// The same, for a **named** surface — what a head's own `Save` button
    /// presses.
    ///
    /// The chord and the button are two doors to one verb, and they name their
    /// surface differently: `Ctrl+S` means "wherever the keyboard is" (the
    /// mock-up's "from any focus state", 6139-6150), a button means the window it
    /// is drawn on. Splitting the naming from the verb is what keeps them one
    /// verb.
    pub(crate) fn save_preview_on(&mut self, surface: PreviewSurface) -> Result<()> {
        if !self.preview_is_editable(surface) {
            return Ok(());
        }
        let Some(buffer) = self.preview_buffer_on_mut(surface) else {
            return Ok(());
        };
        if !buffer.dirty {
            return Ok(());
        }
        let notice = match hang_watch::during(hang_watch::Station::PreviewSave, || buffer.save()) {
            preview::SaveOutcome::Saved => preview::preview_saved_notice().to_owned(),
            preview::SaveOutcome::Conflict => preview::preview_conflict_notice().to_owned(),
            preview::SaveOutcome::Failed(error) => {
                eprintln!("recoverable preview save failure: {error}");
                i18n::not_saved(&error)
            }
        };
        self.preview_pane_mut(surface).notice = Some((notice, Instant::now()));
        self.repaint_preview()
    }

    /// The sentence this surface's body is owed about its last save, if it is
    /// still owed one.
    ///
    /// "Saved" is an acknowledgement and expires after [`FOOT_REVEAL_FEEDBACK`]
    /// (ruling 6: the mock-up's four durations are one). A refusal does not
    /// expire, because it is not a report of something that happened but of
    /// something that did *not*, and a warning that fades is a warning the user
    /// is entitled to have missed.
    fn preview_save_notice(&self, surface: PreviewSurface, now: Instant) -> Option<&str> {
        let (notice, at) = self.preview_pane(surface)?.notice.as_ref()?;
        if notice == preview::preview_saved_notice()
            && now.saturating_duration_since(*at) >= FOOT_REVEAL_FEEDBACK
        {
            return None;
        }
        Some(notice)
    }

    /// The acknowledgement's one wake-up: the **soonest** instant one of them is
    /// due to go away.
    ///
    /// One deadline for every surface, because there is one event loop and it
    /// wakes for the nearest thing owed: two panes each flashing "Saved" are two
    /// words that expire independently, and the earlier of the two is what the
    /// window has to be up for. The sweep at that instant asks all of them again.
    pub(crate) fn preview_notice_deadline(&self) -> Option<Instant> {
        self.preview_panes
            .iter()
            .filter_map(|(_, pane)| pane.notice.as_ref())
            .filter(|(notice, _)| notice == preview::preview_saved_notice())
            .map(|(_, at)| *at + FOOT_REVEAL_FEEDBACK)
            .min()
    }

    /// Take every expired acknowledgement down.
    ///
    /// All of them at once, because the clock the wake-up was set on is the
    /// window's: it fires for the soonest, and a second pane whose word expired
    /// in the same instant must not have to wait for a third event to notice.
    pub(crate) fn advance_preview_notice(&mut self, now: Instant) -> Result<()> {
        let expired: Vec<PreviewSurface> = self
            .preview_panes
            .iter()
            .filter(|(_, pane)| pane.notice.is_some())
            .map(|(surface, _)| surface)
            .filter(|surface| self.preview_save_notice(*surface, now).is_none())
            .collect();
        if expired.is_empty() {
            return Ok(());
        }
        for surface in expired {
            self.preview_pane_mut(surface).notice = None;
        }
        self.repaint_preview()
    }

    /// **Take a refused edit's reason down when its two seconds are up** (owner's
    /// ruling 2026-09-12).
    ///
    /// The acknowledgements' own sweep, one surface along and for its reason:
    /// the deadline above wakes the loop at the instant the pill is due to go,
    /// and a wake with nothing to do repaints nothing — so a pill whose clock had
    /// run out stayed on the glass until something else happened to redraw the
    /// window. Forgotten rather than merely hidden, so that the frame after this
    /// one has no expired state left to ask about.
    pub(crate) fn advance_preview_refusal(&mut self, now: Instant) -> Result<()> {
        let Some((surface, _)) = self.window.preview_refusal else {
            return Ok(());
        };
        if self.preview_refusal_reason(surface, now).is_some() {
            return Ok(());
        }
        self.window.preview_refusal = None;
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// A press inside the edit surface: take the keyboard, put the caret where
    /// the pointer is, and arm the drag that selects.
    ///
    /// Returns whether the press was the surface's.
    pub(crate) fn press_preview_body(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // **A press inside a face that edits is asking to edit** (T2 ③,
        // 2026-09-10) — asked *above* the gate below, because for a file the
        // glance read only the head of, that gate is exactly what this buys the
        // way past. [`Self::ask_to_edit_preview_on`] asks the same question of
        // the face that the gate asks of the body, so a press inside a rendered
        // Markdown page, a table or a diff still reaches no disk.
        //
        // The read is a read: this press does not get a caret out of it, and the
        // next one does. There is no version of "buy the rest of the file" that
        // both answers now and does not put a disk in the gesture.
        if let Some((surface, _)) = self.preview_surface_at(position) {
            self.ask_to_edit_preview_on(surface);
        }
        let Some((surface, body)) = self.preview_edit_body(position) else {
            // **A press that would have seated a caret and could not is a
            // question, and it is owed the answer** (owner's ruling
            // 2026-09-12). The gate one line up is what a read-only body fails,
            // so this is the exact moment a reader learns nothing happened —
            // and before the ruling the reason was standing in a band at the
            // bottom of the pane that they had no cause to be looking at.
            //
            // The press is **not** claimed: it is refused exactly as it was, and
            // the rungs below this one (a rendered page's own press, a link, a
            // selection) go on answering it. All that is added is the sentence.
            if let Some((surface, _)) = self.preview_surface_at(position) {
                self.refuse_preview_edit(surface);
            }
            return Ok(false);
        };
        // **A rendered Markdown page is not this door's surface** (T5,
        // §7.1.3t). Both faces of a `.md` buffer edit now, so
        // `preview_edit_body` — which asks the *buffer* — can no longer tell
        // them apart, and every line below this is the monospace one: an offset
        // is a row times a line height over the whole file, which on a page of
        // proportional blocks names a byte nobody pointed at. The rendered
        // face's own press is [`Self::press_preview_text`], three rungs down the
        // ladder, and it falls through to it by declining here.
        if self.preview_shows_live_markdown(surface) {
            return Ok(false);
        }
        let Some(offset) = self.preview_offset_at(surface, body, scale, position) else {
            return Ok(false);
        };
        let Some(content) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.clone())
        else {
            return Ok(false);
        };
        let mut caret = self.preview_pane_mut(surface).caret;
        // Shift-click extends from wherever the selection already was, which is
        // the one gesture that makes a long selection possible without a drag
        // that outruns the pane.
        caret.place(&content, offset, self.window.modifiers.shift_key());
        self.preview_pane_mut(surface).caret = caret;
        self.preview_edit_focus = Some(surface);
        self.preview_selecting = Some(surface);
        self.repaint_preview()?;
        Ok(true)
    }

    /// A press inside a picture: the second half of a double click, or the start
    /// of a pan (ticket #60).
    ///
    /// Both verbs are armed here rather than one of them at release, because
    /// they are decided by the *press*: a double click is a press that arrives
    /// soon enough after another one, and a pan is every press that is not. The
    /// alternative — waiting for the release to see whether the hand travelled —
    /// would mean the picture does not move until you let go of it.
    ///
    /// A picture is never editable and has no links, so this consumes the press
    /// whole; it lands where [`Self::press_preview_body`] would have refused and
    /// the press would have died as "inside a seat with no grid" anyway.
    pub(crate) fn press_preview_image(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some((surface, _)) = self.preview_surface_at(position) else {
            return Ok(false);
        };
        if !self.picture_takes_zoom(surface) || self.preview_image_geometry(surface).is_none() {
            return Ok(false);
        }
        let point = [position.x as f32, position.y as f32];
        if self
            .preview_image_clicks
            .register(surface, point, Instant::now())
        {
            let toggled = image_zoom_toggled(self.preview_image_zoom(surface));
            self.set_preview_image_zoom(surface, toggled)?;
        } else {
            self.preview_image_drag = Some(ImageDrag {
                surface,
                last: point,
            });
        }
        self.apply_pointer_cursor();
        Ok(true)
    }

    /// The pointer travelling with a picture in hand.
    ///
    /// The gesture's own surface, not the one under the pointer — the rule every
    /// drag on this desk follows, and the reason a pan that outruns the pane
    /// keeps panning instead of jumping to whatever is next door.
    pub(crate) fn drag_preview_image(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(drag) = self.preview_image_drag else {
            return Ok(false);
        };
        let Some((body, image_px)) = self.preview_image_geometry(drag.surface) else {
            return Ok(true);
        };
        let point = [position.x as f32, position.y as f32];
        let zoom = self.preview_image_zoom(drag.surface);
        let carried = ImageZoom {
            pan: [
                zoom.pan[0] + point[0] - drag.last[0],
                zoom.pan[1] + point[1] - drag.last[1],
            ],
            ..zoom
        };
        self.preview_image_drag = Some(ImageDrag {
            last: point,
            ..drag
        });
        let clamped = ImageZoom {
            pan: image_clamped_pan(body, image_px, carried),
            ..carried
        };
        self.set_preview_image_zoom(drag.surface, clamped)?;
        Ok(true)
    }

    /// The pointer travelling with the button down, mid-selection.
    pub(crate) fn drag_preview_selection(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        // The gesture's own surface, not the one under the pointer: a selection
        // belongs to the body it began in however far the hand has since gone.
        let Some(surface) = self.preview_selecting else {
            return Ok(false);
        };
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(body) = self.preview_surface_body_rect(surface, scale) else {
            return Ok(false);
        };
        let Some(offset) = self.preview_offset_at(surface, body, scale, position) else {
            return Ok(false);
        };
        let Some(content) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.clone())
        else {
            return Ok(false);
        };
        let was = self.preview_pane_mut(surface).caret;
        let mut caret = was;
        caret.place(&content, offset, true);
        if caret == was {
            return Ok(true);
        }
        self.preview_pane_mut(surface).caret = caret;
        self.repaint_preview()?;
        Ok(true)
    }

    /// The edit surface the pointer is inside, and its rectangle.
    pub(crate) fn preview_edit_body(
        &self,
        position: PhysicalPosition<f64>,
    ) -> Option<(PreviewSurface, [f32; 4])> {
        let (surface, body) = self.preview_surface_at(position)?;
        self.preview_is_editable(surface).then_some((surface, body))
    }

    /// Which byte of this surface's body a point names.
    ///
    /// **The painter's own arithmetic, read backwards.** The row is the line the
    /// point is inside and the column is the *nearest* cell boundary rather than
    /// the one it is inside — a click on the right half of a character puts the
    /// caret after it, which is what makes clicking at the end of a line land at
    /// the end of the line.
    fn preview_offset_at(
        &self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
        position: PhysicalPosition<f64>,
    ) -> Option<usize> {
        let content = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.as_deref())?;
        let pane = self.preview_pane(surface)?;
        let metrics = seats::preview_text_metrics(scale);
        let advance = pane.mono_advance;
        if advance <= 0.0 {
            return None;
        }
        let x = position.x as f32 - body[0] - metrics.padding_x + pane.scroll[0];
        let y = position.y as f32 - body[1] - metrics.padding_y + pane.scroll[1];
        let row = (y / metrics.line_height).floor().max(0.0) as usize;
        // Cells and not a cell, for [`preview_edit::byte_at_x`]'s reason: which
        // side of a two-cell character a press belongs to is a question a
        // rounded column has already thrown the answer to away.
        let columns = (x / advance).max(0.0);
        // **The painter's arithmetic read backwards, through the same wrap.** A
        // click names a *drawn* row; which line that is and how far into it the
        // row starts is exactly what the layout knows, and asking it here is
        // what keeps the caret under the pointer on a reflowed line.
        let (line, from, to) = self
            .preview_wrap(surface)
            .and_then(|wrap| wrap.row_span(row))
            .map_or((row, 0, usize::MAX), |span| span);
        #[allow(clippy::cast_precision_loss)]
        let columns = (from as f32 + columns).min(to as f32);
        Some(preview_edit::offset_at_x(content, line, columns))
    }

    /// How this surface's text is currently wrapped, if it is showing text.
    fn preview_wrap(&self, surface: PreviewSurface) -> Option<&preview_edit::WrapLayout> {
        match &self.preview_pane(surface)?.doc {
            PreviewDocument::Text { wrap, .. } => Some(wrap),
            _ => None,
        }
    }

    /// The body has changed and the window owes a frame for it.
    pub(crate) fn repaint_preview(&mut self) -> Result<()> {
        self.refresh_preview_body();
        self.refresh_chrome();
        // The caret has very likely just moved, and the candidate list has to
        // follow it. Kept here as well as in the loop's tail because this door
        // is *earlier*: the body has already been rebuilt, so the rectangle is
        // the new one, and the candidate window moves with the same present the
        // letters do rather than one wake-up behind them.
        self.offer_ime_caret(None);
        self.present_chrome_change()
    }

    /// The only scroll a preview body is allowed to hold — **every** surface's.
    ///
    /// `heal_files_scroll`'s twin and its whole argument: the painter believes
    /// the stored number, so a buffer that got shorter, a pane that got taller
    /// or a switch to a smaller file has to be answered *here* rather than by
    /// the layout quietly disagreeing with what it was handed. All of them,
    /// because one solve moves every rectangle at once: healing only the pane the
    /// gesture touched would leave the others believing a geometry that stopped
    /// existing in the same frame.
    ///
    /// **Reports whether it moved anything**, which is [`Self::settle_preview_goto`]'s
    /// closing sentence said about the other writer of the same number: the body
    /// is built *from* the offset, so an offset written after it was built is a
    /// number nothing has drawn. Left unsaid, a pane grown taller or a document
    /// grown shorter kept the frame that had been built past its own end — a body
    /// with nothing in it — until some unrelated event rebuilt it, which on the
    /// glass is a blank pane that one notch of the wheel fixes for good.
    fn heal_preview_scroll(&mut self) -> bool {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let mut moved = false;
        for surface in self.preview_surfaces() {
            let Some(body) = self.preview_surface_body_rect(surface, scale) else {
                continue;
            };
            let Some(scroll) = self.preview_pane(surface).map(|pane| pane.scroll) else {
                continue;
            };
            let healed = self.clamped_preview_scroll(surface, body, scale, scroll);
            if healed != scroll {
                self.preview_pane_mut(surface).scroll = healed;
                moved = true;
            }
        }
        moved
    }

    /// **The band of pictures this surface is looking at** — [`PictureReach`],
    /// read off the layout the surface is already standing on.
    ///
    /// [`PictureReach::from_the_top`] when there is none to read: a surface that
    /// is not showing a page, or one whose page has just been parsed and not yet
    /// laid out. Both are a reader at the top of a document, which is what that
    /// answer says.
    ///
    /// The layout may belong to the *previous* document on this surface, and
    /// that is harmless rather than overlooked: a page arriving on a surface
    /// arrives at the top of itself, so the band read off the old boxes is the
    /// band at the old scroll, and the pass after this one — the first with a
    /// layout of its own — corrects it through the key.
    fn standing_picture_reach(&self, surface: PreviewSurface, body: [f32; 4]) -> PictureReach {
        let Some(pane) = self.preview_pane(surface) else {
            return PictureReach::from_the_top();
        };
        let PreviewDocument::Markdown { wrap, layout, .. } = &pane.doc else {
            return PictureReach::from_the_top();
        };
        wrap.viewport
            .picture_reach(layout, pane.scroll[1], body[3] - body[1])
    }

    /// Re-derive the parsed body if what it was parsed from has changed.
    ///
    /// The content is cloned once here rather than borrowed, which is what lets
    /// the measuring below hold the renderer: a rebuild happens when a file
    /// lands or a pane changes width, and paying one copy for that is the whole
    /// price of never copying on a wheel notch.
    pub(crate) fn rebuild_preview_document(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
    ) {
        // The face this surface is showing is part of the key, because the two
        // faces of one markdown file are two documents — and now that the flip
        // is the view's, two surfaces on one file can be caching both at once.
        let md_source = self.preview_md_source(surface);
        let math_generation = self.window.preview_math.generation;
        let body_ink = bt_render::chrome_palette().files_row_text;
        let picture_generation = self.window.markdown_pictures.generation;
        let theme = bt_render::current_theme();
        // **Where the document is** — a picture's source is relative to it
        // (§7.1.3k ①), and it is read here rather than at resolve time because
        // this is where the buffer is already in hand.
        let document = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.source.file_path().map(Path::to_path_buf));
        // **Where the reader is, in pictures** (review row R1-8). Read off the
        // layout this surface is already standing on, because that is the only
        // thing that knows where a block ended up — and it goes into the key, so
        // that scrolling into the next band is a re-flow the same way a formula
        // landing is.
        let picture_reach = self.standing_picture_reach(surface, body);
        let art_key = PageArtKey {
            math_generation,
            body_ink,
            picture_generation,
            picture_reach,
            theme,
        };
        // **Which block the caret is in** (§7.1.3q). Read before the key,
        // because it is part of it — and read off the document *already* on this
        // surface, which is the only parse whose ranges describe the bytes the
        // caret is an offset into. When the content has moved under it, the
        // answer below is stale and the key differs on its revision anyway; the
        // parse that follows fills the true one in.
        let live_caret = self.preview_live_caret(surface);
        let standing_source = self.standing_source_block(surface, live_caret);
        let mut key = self.preview_buffer_on(surface).map(|buffer| {
            preview_document_key(
                buffer,
                md_source,
                body[2] - body[0],
                scale,
                art_key,
                standing_source.clone(),
            )
        });
        if let Some(key) = key.as_mut() {
            key.font_environment_epoch = self.app.gpu.font_environment_epoch();
        }
        let exact_frame = {
            let metrics = seats::preview_markdown_metrics(scale);
            let (left, right) = preview::markdown_measure_box(body, metrics);
            let frame = preview_wrap::Frame::new(
                right - left,
                scale,
                self.app.gpu.font_environment_epoch(),
            );
            self.preview_pane(surface)
                .is_none_or(|pane| match &pane.doc {
                    PreviewDocument::Markdown { wrap, .. } => wrap.frame == Some(frame),
                    _ => true,
                })
        };
        if exact_frame && key == self.preview_pane_mut(surface).doc_key {
            self.ensure_markdown_viewport(surface, body, scale);
            return;
        }
        let reach_only = exact_frame
            && key
                .as_ref()
                .zip(self.preview_pane(surface).and_then(|p| p.doc_key.as_ref()))
                .is_some_and(|(new, old)| {
                    let mut old = old.clone();
                    old.art.picture_reach = new.art.picture_reach;
                    old == *new
                });
        if reach_only {
            self.update_markdown_picture_reach(
                surface,
                body,
                scale,
                document.as_deref(),
                picture_reach,
            );
            self.preview_pane_mut(surface).doc_key = key;
            return;
        }
        let same_document = key
            .as_ref()
            .zip(
                self.preview_pane(surface)
                    .and_then(|pane| pane.doc_key.as_ref()),
            )
            .is_some_and(|(new, old)| new.parse.source == old.parse.source);
        let viewport_edits = if same_document {
            self.preview_pane(surface)
                .and_then(|p| p.doc_key.as_ref())
                .and_then(|old| {
                    self.preview_buffer_on(surface)?
                        .viewport_edits_since(old.parse.revision)
                })
        } else {
            None
        };
        // **What this page has already been told about its art**, taken before
        // the document it is written in is replaced. It is the ledger that makes
        // an answer an answer when a bounded cache has let the pixels go — see
        // [`answer_one_picture`] for the pictures and [`answer_one_formula`] for
        // the formulas, which are one rule about two lanes. Read after the key's
        // own early return, so a window that is not rebuilding pays nothing.
        let (standing_pictures, standing_math) = self
            .preview_pane(surface)
            .and_then(|pane| match &pane.doc {
                PreviewDocument::Markdown { pictures, math, .. } => {
                    Some((pictures.clone(), math.clone()))
                }
                _ => None,
            })
            .unwrap_or_default();
        // **A resize re-flows; it does not re-parse** (user report, 2026-08-13).
        // The parse and every measurement that does not depend on the pane's
        // width are keyed on the content alone, so dragging a window edge pays
        // for the one pass that is genuinely per-width — how many lines the
        // wrapping blocks take — instead of re-parsing 64KB and re-shaping every
        // table cell sixty times a second. See [`MarkdownBlockIntrinsic`].
        let pane = self.preview_pane_mut(surface);
        let reflow_only =
            key.as_ref().map(|key| &key.parse) == pane.doc_key.as_ref().map(|key| &key.parse);
        // **Did the bytes actually move?** Read here, while the key being
        // replaced is still standing, and spent at the bottom of this function
        // on the one question it decides — see [`Reparse`].
        let content_moved = key.as_ref().map(|key| key.parse.revision)
            != pane.doc_key.as_ref().map(|key| key.parse.revision);
        // **A formula arriving is not a resize**, and the one intrinsic that
        // notices is a table's columns: a cell holding a formula is as wide as
        // that formula's picture, and a column measured before the picture
        // existed reserved the width of the LaTeX instead. Re-measured here and
        // nowhere else, because this is the only moment the answer can have
        // changed with the parse standing still — and it is rare (once per
        // picture, not once per pixel of a drag), which is exactly why it can
        // afford the pass a resize cannot.
        let art_changed = key.as_ref().map(|key| {
            (
                key.art.math_generation,
                key.art.body_ink,
                key.art.picture_generation,
            )
        }) != pane.doc_key.as_ref().map(|key| {
            (
                key.art.math_generation,
                key.art.body_ink,
                key.art.picture_generation,
            )
        });
        pane.doc_key = key;
        if reflow_only && matches!(pane.doc, PreviewDocument::Markdown { .. }) {
            let old = std::mem::take(&mut pane.doc);
            let PreviewDocument::Markdown {
                blocks,
                ranges,
                maps,
                ..
            } = &old
            else {
                unreachable!()
            };
            let metrics = seats::preview_markdown_metrics(scale);
            let (left, right) = preview::markdown_measure_box(body, metrics);
            let source =
                self.markdown_caret_block(surface, standing_source.as_ref(), blocks, scale);
            let math = self.resolve_document_math(
                blocks,
                metrics,
                &bt_render::chrome_palette(),
                &standing_math,
            );
            let pictures = self.resolve_document_pictures(
                blocks,
                document.as_deref(),
                right - left,
                picture_reach,
                &standing_pictures,
            );
            let content = self
                .preview_buffer_on(surface)
                .and_then(|b| b.content.clone())
                .unwrap_or_default();
            let (layout, intrinsic, wrap) = self.rebuild_markdown_geometry(
                surface,
                body,
                scale,
                &old,
                preview_viewport::Build {
                    blocks,
                    maps,
                    bytes: MarkdownSourceBytes {
                        content: &content,
                        ranges,
                    },
                    source: source.as_deref(),
                    art: PageArt {
                        math: &math,
                        pictures: &pictures,
                        theme,
                    },
                    edits: viewport_edits.as_deref(),
                    art_changed,
                },
            );
            let PreviewDocument::Markdown {
                blocks,
                ranges,
                maps,
                ..
            } = old
            else {
                unreachable!()
            };
            self.preview_pane_mut(surface)
                .reflow_document(PreviewDocument::Markdown {
                    blocks,
                    ranges,
                    maps,
                    source,
                    intrinsic,
                    layout,
                    math,
                    pictures,
                    wrap,
                });
            return;
        }
        let Some((view, content, name)) = self.preview_buffer_on(surface).map(|buffer| {
            (
                buffer.view(md_source),
                buffer.content.clone().unwrap_or_default(),
                buffer.name.clone(),
            )
        }) else {
            self.preview_pane_mut(surface)
                .show_document(PreviewDocument::Empty, Reparse::Elsewhere);
            return;
        };
        let text_metrics = seats::preview_text_metrics(scale);
        let advance = self
            .window
            .renderer
            .preview_mono_advance(&mut self.app.gpu, text_metrics.font_size);
        self.preview_pane_mut(surface).mono_advance = advance;
        // Filled in by the markdown arm alone, and written back into the key
        // under the match: every other view has no blocks and therefore no block
        // the caret could be in.
        let mut parsed_source: Option<(usize, std::ops::Range<usize>)> = None;
        let doc = match view {
            // The editor's own line model, not [`str::lines`]: a body ending in
            // a break has an empty line after it, the caret can stand on that
            // line, and a document that did not draw it would be a document a
            // caret could leave.
            preview::PreviewView::Text => {
                let lines = preview_edit::display_lines(&content);
                let wrap = match preview_wrap_columns(body, text_metrics, advance) {
                    Some(columns) => preview_edit::WrapLayout::wrapped(&lines, columns),
                    None => preview_edit::WrapLayout::unwrapped(&lines),
                };
                // **#49, and it belongs on this side of the key.** The grammar
                // is chosen from the file's own name and its first line, and the
                // walk is a fact about the content — neither has anything to do
                // with how wide the pane is, so both are paid for here, once,
                // with the parse. A file whose language is not in the box comes
                // back plain, which is the same [`highlight::Highlighting`] an
                // over-cap file comes back as and the same one this document
                // carried before highlighting existed.
                let highlight =
                    highlight::syntax_for_file(&name, lines.first().map(String::as_str))
                        .map(|grammar| highlight::Highlighting::of(&lines, grammar))
                        .unwrap_or_default();
                PreviewDocument::Text {
                    lines,
                    wrap,
                    highlight,
                }
            }
            preview::PreviewView::Diff => {
                let metrics = seats::preview_diff_metrics(scale);
                let margin = (seats::PREVIEW_DIFF_HUNK_MARGIN_LOGICAL_PX * scale).round();
                let mut top = 0.0_f32;
                let rows = content
                    .lines()
                    .map(|line| {
                        let kind = preview::diff_line_kind(line);
                        // A hunk marker opens a gap in front of itself, and the
                        // gap belongs to the rows below it too — which is why
                        // this is a running offset and not a per-row nudge.
                        if kind == preview::DiffLineKind::Hunk {
                            top += margin;
                        }
                        let row = DiffRow {
                            text: preview::expand_tabs(line),
                            kind,
                            top,
                        };
                        top += metrics.line_height;
                        row
                    })
                    .collect();
                PreviewDocument::Diff(rows)
            }
            preview::PreviewView::Table => {
                let rows = preview::csv_rows(&content);
                let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
                let column_cells = (0..columns)
                    .map(|column| {
                        rows.iter()
                            .filter_map(|row| row.get(column))
                            .map(|cell| bt_unicode::text_width(cell))
                            .max()
                            .unwrap_or(0)
                    })
                    .collect();
                PreviewDocument::Table { rows, column_cells }
            }
            preview::PreviewView::Markdown => {
                let metrics = seats::preview_markdown_metrics(scale);
                let clock = preview_trace::global().map(|_| Instant::now());
                // **The maps come off the same walk** (§7.1.3r): a click on this
                // page has to name a byte of the file, and asking a second parse
                // for the answer would be a second parse per press.
                let (blocks, ranges, maps) = preview::parse_markdown_mapped(&content);
                let parsed = clock.map(|clock| clock.elapsed());
                let (measure_left, measure_right) = preview::markdown_measure_box(body, metrics);
                let width = (measure_right - measure_left).max(1.0);
                // **The caret's own block, now that there is a parse to index**
                // (§7.1.3q). Written back into the key below, because the key
                // was built before this parse existed and a key that said `None`
                // where the document says `Some` would re-lay-out the whole page
                // on the very next frame.
                parsed_source = live_caret
                    .and_then(|caret| {
                        preview_live::caret_seat(&content, &ranges, caret.caret).block()
                    })
                    .and_then(|index| Some((index, ranges.get(index)?.clone())));
                let source =
                    self.markdown_caret_block(surface, parsed_source.as_ref(), &blocks, scale);

                let math = self.resolve_document_math(
                    &blocks,
                    metrics,
                    &bt_render::chrome_palette(),
                    &standing_math,
                );
                let pictures = self.resolve_document_pictures(
                    &blocks,
                    document.as_deref(),
                    width,
                    picture_reach,
                    &standing_pictures,
                );
                let clock = clock.map(|_| Instant::now());
                let old = std::mem::take(&mut self.preview_pane_mut(surface).doc);
                let (layout, intrinsic, wrap) = self.rebuild_markdown_geometry(
                    surface,
                    body,
                    scale,
                    &old,
                    preview_viewport::Build {
                        blocks: &blocks,
                        maps: &maps,
                        bytes: MarkdownSourceBytes {
                            content: &content,
                            ranges: &ranges,
                        },
                        source: source.as_deref(),
                        art: PageArt {
                            math: &math,
                            pictures: &pictures,
                            theme,
                        },
                        edits: viewport_edits.as_deref(),
                        art_changed,
                    },
                );
                // Lazy intrinsics are included in this viewport layout batch.
                let measured = clock.map(|_| std::time::Duration::ZERO);
                if let Some((((parsed, measured), laid), trace)) = parsed
                    .zip(measured)
                    .zip(clock.map(|clock| clock.elapsed()))
                    .zip(preview_trace::global())
                {
                    preview_trace::document(
                        Some(trace),
                        preview_trace::DocumentBuild {
                            bytes: content.len(),
                            blocks: blocks.len(),
                            source: parsed_source.as_ref().map(|(index, _)| *index),
                            hits: self.window.markdown_intrinsics.hits,
                            misses: self.window.markdown_intrinsics.misses,
                            parse: parsed,
                            intrinsic: measured,
                            layout: laid,
                        },
                    );
                }
                PreviewDocument::Markdown {
                    blocks,
                    ranges,
                    maps,
                    source,
                    intrinsic,
                    layout,
                    math,
                    pictures,
                    wrap,
                }
            }
            // **The graph's body is empty on purpose**, and it is the one place
            // in this match where that is a statement rather than an absence:
            // the picture is drawn by the chrome into this pane's own body
            // rectangle, so a `PreviewDocument` that put anything here would put
            // it *over* the graph. An image is the same arrangement one surface
            // along, and `None` is the card.
            // And a page's body is empty for the graph's reason one lane over:
            // the pixels are the engine's, composed *under* this surface and seen
            // through the hole punched in it (DESIGN.md 7.8 (2)), so anything put
            // here would be put over a browser.
            // A video is the image's arrangement exactly: a frame on the picture
            // channel, with its facts at the right end of its own bar
            // (`video_meta_sentence`) since the ruling of 2026-09-12, where a
            // picture's are at the right end of the path row
            // (`preview_meta_sentence`). Neither is a document parsed into this.
            preview::PreviewView::Image
            | preview::PreviewView::Video
            | preview::PreviewView::Graph
            | preview::PreviewView::Web
            | preview::PreviewView::None => PreviewDocument::Empty,
        };
        // **Whose bytes these are** (§7.1.3q; research open question 15). The
        // one call site that can answer it, because it is the one that holds
        // both keys: the content moved only if the revision did, and the buffer
        // says whether the hand that moved it was in this window. A re-parse
        // with the revision standing — the face flipped, the window crossed a
        // monitor — is nobody's doing and drops nothing.
        let ours = self
            .preview_buffer_on(surface)
            .is_some_and(preview::PreviewBuffer::was_edited_here);
        let reparse = if content_moved && !ours {
            Reparse::Elsewhere
        } else {
            Reparse::Ours
        };
        if let Some(key) = self.preview_pane_mut(surface).doc_key.as_mut() {
            key.source = parsed_source;
        }
        // Drag and hover identities are positional too; a new parse retires them.
        if self
            .preview_block_drag
            .is_some_and(|drag| drag.surface == surface)
        {
            self.preview_block_drag = None;
        }
        if self
            .preview_block_hover
            .is_some_and(|(over, _)| over == surface)
        {
            self.preview_block_hover = None;
        }
        self.preview_pane_mut(surface).show_document(doc, reparse);
    }

    /// **The caret standing in a rendered markdown document, when there is one**
    /// (§7.1.3q).
    ///
    /// `None` on every surface that cannot take a keystroke, and on every page
    /// nobody has put a caret in: **entering is a press in the body**
    /// ([`PreviewPane::md_caret`], §7.1.3t), so a page being read is a page with
    /// no source block in it, drawn exactly as it was before this feature
    /// existed.
    ///
    /// **Not gated on the keyboard focus**, and that is deliberate: which block
    /// is drawn as source is a property of the *document*, and a page that
    /// re-flowed itself every time the reader clicked into a terminal and back
    /// would be a page that moves under a hand that is not on it. What the focus
    /// decides is whether the caret is *drawn* ([`MarkdownCaretPaint::lit`]),
    /// which is the text face's own rule. Leaving on purpose — `Esc` — is what
    /// puts the block back, and it does it by clearing the flag rather than by
    /// being a second kind of focus.
    pub(crate) fn preview_live_caret(
        &self,
        surface: PreviewSurface,
    ) -> Option<preview_edit::EditCaret> {
        if !self.preview_pane(surface)?.md_caret {
            return None;
        }
        let md_source = self.preview_md_source(surface);
        if md_source {
            return None;
        }
        let buffer = self.preview_buffer_on(surface)?;
        if buffer.view(md_source) != preview::PreviewView::Markdown
            || !buffer.is_editable(md_source)
        {
            return None;
        }
        Some(self.preview_pane(surface)?.caret)
    }

    /// **The caret's block, cut out of the buffer and dressed in the face its
    /// kind wears** (§7.1.3q for the one, §7.1.3w for the other).
    ///
    /// Built from the buffer rather than from the parse, because the whole point
    /// of the caret's block is that it is **the file's own bytes** and not a
    /// rendering of them: the block beside it in `blocks` has already lost its
    /// hashes, its pipes and its indent.
    ///
    /// **The parse decides only the face.** A heading, a paragraph, a list and a
    /// quote keep the body face they were read in and show their marks in it; a
    /// fence, a table, a display formula and a rule turn monospace, because for
    /// those the alignment is the content and monospace is the honest face for
    /// it. Nothing else about this depends on the kind — the bytes, the range
    /// and the line breaks are the same bytes, range and breaks either way.
    fn markdown_caret_block(
        &self,
        surface: PreviewSurface,
        source: Option<&(usize, std::ops::Range<usize>)>,
        blocks: &[preview::MarkdownBlock],
        scale: f32,
    ) -> Option<Box<MarkdownCaretBlock>> {
        let (index, range) = source?;
        let content = self.preview_buffer_on(surface)?.content.as_deref()?;
        let text = preview_live::block_source(content, range).to_owned();
        let Some(heading) = markdown_prose_face(blocks.get(*index)?) else {
            let metrics = seats::preview_text_metrics(scale);
            return Some(Box::new(MarkdownCaretBlock::Mono(MarkdownSourceBlock {
                index: *index,
                range: range.clone(),
                lines: preview_edit::display_lines(&text),
                text,
                font_size: metrics.font_size,
                line_height: metrics.line_height,
                advance: self
                    .preview_pane(surface)
                    .map_or(0.0, |pane| pane.mono_advance),
            })));
        };
        let metrics = seats::preview_markdown_metrics(scale);
        let (font_size, line_height) = match heading {
            Some(level) => (
                metrics.heading_font(level),
                metrics.heading_line_height(level),
            ),
            None => (metrics.font_size, metrics.line_height),
        };
        Some(Box::new(MarkdownCaretBlock::Prose(MarkdownProseBlock {
            index: *index,
            range: range.clone(),
            lines: prose_source_lines(&text),
            text,
            heading: heading.is_some(),
            font_size,
            line_height,
        })))
    }

    /// **Find every picture this page needs, and ask for the ones that are
    /// missing** (user ruling 2026-08-28; §7.1.3k).
    ///
    /// [`Self::resolve_document_math`]'s twin, and the same two halves in one
    /// pass for the same reason: the walk that collects the pictures is the walk
    /// that discovers the gaps, and a separate "request" pass would need a rule
    /// for when to run it that does not exist.
    ///
    /// **Two lanes answer, in this order.** The decode lane
    /// ([`Self::request_peek_pixels`], `peek_cache`) says what the file's own
    /// pixels are — that is what the block's shape comes from. The resample lane
    /// then says what those pixels look like at the size the page draws them,
    /// and until it has said so the decode's own raster is handed over and the
    /// sampler stretches it. Nothing is ever *absent* while it sharpens.
    ///
    /// **The exact-size pass waits for the quiet** (§7.1.3j (d), applied to a
    /// page): a window drag walks a document through a hundred widths, and a
    /// Lanczos3 pass per width is a hundred passes for ninety-nine sizes the
    /// hand has already left. What is owed is written down by content key, so
    /// every width the drag passes through overwrites the same entry, and
    /// [`Self::finish_preview_scale_if_quiet`] sends the one that was current
    /// when the hand stopped.
    pub(crate) fn resolve_document_pictures(
        &mut self,
        blocks: &[preview::MarkdownBlock],
        document: Option<&Path>,
        measure_px: f32,
        reach: PictureReach,
        standing: &DocumentPictures,
    ) -> DocumentPictures {
        let theme = bt_render::current_theme();
        let now = Instant::now();
        self.window.markdown_pictures.tick = self.window.markdown_pictures.tick.saturating_add(1);
        let mut ask =
            |path: &Path, fill: bool, standing: Option<&MarkdownPicture>| -> PagePicture {
                // The two caches are borrowed for exactly as long as the answer
                // takes; the door below wants the whole runtime, so it is spent
                // after those borrows have ended — see [`answer_one_picture`].
                let mut needs_pixels = false;
                let answer = answer_one_picture(
                    &mut self.window.peek_cache,
                    &mut self.window.markdown_pictures,
                    standing,
                    path,
                    fill,
                    measure_px,
                    now,
                    &mut needs_pixels,
                );
                if !needs_pixels {
                    return answer;
                }
                if self.request_peek_pixels(path) {
                    preview_trace::picture_read(preview_trace::global(), path);
                    self.window.peek_cache.insert(
                        bt_term::normalized_local_image_path_key(path),
                        PeekCacheEntry::Pending,
                    );
                    return answer;
                }
                // A file this window will not open draws what a picture it cannot
                // read draws — unless the page already has something true to show,
                // which a refused *resample* leaves standing. Either way it is
                // waiting for nothing: nobody was asked, so nothing is coming.
                match answer.picture {
                    MarkdownPicture::Loading => PagePicture::drawn(MarkdownPicture::Failed),
                    picture => PagePicture::drawn(picture),
                }
            };
        resolve_document_pictures(blocks, document, theme, reach, standing, &mut ask)
    }

    /// Send every exact-size pass the quiet has released.
    ///
    /// Answers whether anything went out, which is what owes the caller a
    /// re-flow: a page whose pictures are on their way has not changed yet, but
    /// the ledger it is read from has.
    fn send_owed_markdown_rasters(&mut self) -> bool {
        if !self.app.math_worker_running {
            self.window.markdown_pictures.owed.clear();
            self.window.markdown_pictures.settle_deadline = None;
            return false;
        }
        let owed: Vec<MarkdownRasterRequest> = self
            .window
            .markdown_pictures
            .owed
            .drain()
            .map(|(_, request)| request)
            .collect();
        self.window.markdown_pictures.settle_deadline = None;
        let leaf = self.focused_shell_address();
        let mut sent = false;
        for request in owed {
            let task = peek_scale_task(
                &(
                    request.key.content.clone(),
                    request.key.width_px,
                    request.key.height_px,
                ),
                request.rgba,
                request.native[0],
                request.native[1],
            );
            if self
                .app
                .math_worker
                .scale_tasks
                .send(ScaleWorkerRequest::MarkdownImage { leaf, task })
                .is_ok()
            {
                self.window
                    .markdown_pictures
                    .land(request.key, MarkdownRaster::Pending);
                sent = true;
            }
        }
        sent
    }

    /// Take delivery of one exact-size markdown raster.
    pub(crate) fn complete_markdown_raster(&mut self, scaled: bt_term::ScaledInlineImage) {
        let key = MarkdownRasterKey {
            content: scaled.content_key.clone(),
            width_px: scaled.width_px,
            height_px: scaled.height_px,
        };
        self.window.markdown_pictures.land(
            key,
            MarkdownRaster::Ready {
                key: scaled.key,
                rgba: scaled.rgba,
                width_px: scaled.width_px,
                height_px: scaled.height_px,
            },
        );
    }

    /// How tall the parsed document is, and how wide, in the units the scroller
    /// is clamped in.
    fn preview_content_extent(&self, surface: PreviewSurface, scale: f32) -> (f32, usize) {
        let Some(pane) = self.preview_pane(surface) else {
            return (0.0, 0);
        };
        match &pane.doc {
            PreviewDocument::Empty => (0.0, 0),
            // **A folded body is as tall as its rows**, not as its lines. The
            // width it answers is still the file's own; whether that width is
            // *reachable* is one ruling made in one place, and that place is
            // `preview_document_max_scroll`.
            PreviewDocument::Text { wrap, .. } => (
                seats::preview_text_metrics(scale).line_height * wrap.rows() as f32,
                self.preview_buffer_on(surface)
                    .map_or(0, |buffer| buffer.max_columns),
            ),
            PreviewDocument::Diff(rows) => (
                rows.last().map_or(0.0, |row| {
                    row.top + seats::preview_diff_metrics(scale).line_height
                }),
                self.preview_buffer_on(surface)
                    .map_or(0, |buffer| buffer.max_columns),
            ),
            PreviewDocument::Table { .. } | PreviewDocument::Markdown { .. } => (0.0, 0),
        }
    }

    /// [`preview_document_height`] for the document **this surface** is holding.
    pub(crate) fn preview_surface_document_height(
        &self,
        surface: PreviewSurface,
        body: [f32; 4],
        scale: f32,
    ) -> f32 {
        let Some(pane) = self.preview_pane(surface) else {
            return 0.0;
        };
        let advance = pane.mono_advance;
        let (rows_height, columns) = self.preview_content_extent(surface, scale);
        preview_document_height(&pane.doc, body, scale, advance, rows_height, columns)
    }

    /// Hand the renderer the body of every preview **seat** this tab has.
    ///
    /// One list rather than one slot, because the content plane is plural: two
    /// preview panes are two documents at two scroll positions, and a renderer
    /// holding one body could only ever draw the last one built. The order is
    /// [`Self::preview_surfaces`]'s, which is stable between frames — a list
    /// whose order changed would be a diff against the last frame that never
    /// settles.
    ///
    /// **A float's document is deliberately not in this list.** This lane is
    /// drawn a whole pass *before* the overlays, so a body handed to it would be
    /// painted behind the very window that contains it; a preview float's
    /// document rides on its own [`marks::OverlayLayer::body`] instead — see
    /// [`Self::preview_float_layer`].
    pub(crate) fn refresh_preview_body(&mut self) {
        let bodies = self
            .preview_surfaces()
            .into_iter()
            .filter(|surface| matches!(surface, PreviewSurface::Seat(_)))
            .filter_map(|surface| self.build_preview_body(surface))
            .collect();
        self.window.renderer.set_preview_bodies(bodies);
    }

    /// The body of whatever **one** surface is showing, or nothing when it is
    /// showing nothing at all.
    ///
    /// **Only what is visible is built.** A 64KB head is some two thousand lines
    /// and a pane holds perhaps forty; building the rest would put the file's
    /// size into the frame's cost, which is exactly what the head read exists to
    /// keep out of it.
    fn build_preview_body(&mut self, surface: PreviewSurface) -> Option<bt_render::PreviewBody> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(body) = self.preview_surface_body_rect(surface, scale) else {
            // The seat has no solved rectangle this pass — which is a body that
            // is not drawn at all, and on screen is a pane showing its head, its
            // foot and the two hairlines between them.
            preview_trace::emit(preview_trace::global(), || {
                format!("built {surface:?} leave=no-rect")
            });
            return None;
        };
        self.build_preview_body_in(surface, body)
    }

    /// The same, in a rectangle the caller already knows.
    ///
    /// The split is what the glance card needed. A seat and a float are asked
    /// where their body is; the card's body is a box the card's own layout
    /// computed, from a height that depends on how tall this very document turns
    /// out to be. Handing the rectangle in is what breaks that circle without
    /// giving the card a second renderer.
    pub(crate) fn build_preview_body_in(
        &mut self,
        surface: PreviewSurface,
        body: [f32; 4],
    ) -> Option<bt_render::PreviewBody> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // A picture's own lane draws the pixels; what is left for this surface
        // is the sentence under them (mock-up 4955).
        if self
            .preview_pane(surface)
            .is_some_and(|pane| pane.image.is_some())
        {
            preview_trace::emit(preview_trace::global(), || {
                format!("built {surface:?} leave=picture")
            });
            // **And nothing under it since 2026-09-12** (owner's ruling): the
            // sentence that used to stand here is at the right end of the path
            // row for a picture (`preview_meta_sentence`) and at the right end
            // of the bar for a recording (`video_meta_sentence`). What is left
            // for this lane is the play button.
            let mut built: Option<bt_render::PreviewBody> = None;
            // **The play button rides here and not with the seat's own chrome**
            // (user ruling 2026-08-27; §7.23 ⑩), and the reason was measured on
            // the machine that afternoon: the seats' sprite pass is issued
            // *before* the preview picture, so the first build drew a pill and a
            // triangle and then painted the decoded frame straight over them —
            // a video's pane with a bare rectangle where its play button was.
            // This lane is the one drawn immediately after the picture, and its
            // own definition is a textured quad in whole-surface pixels cropped
            // to a box, which is exactly what these two are.
            let rasters = self.play_button_rasters(surface, body, scale);
            if !rasters.is_empty() {
                built
                    .get_or_insert_with(|| bt_render::PreviewBody {
                        clip: body,
                        quads: Vec::new(),
                        paragraphs: Vec::new(),
                        blocks: Vec::new(),
                        rasters: Vec::new(),
                    })
                    .rasters
                    .extend(rasters);
            }
            return built;
        }
        // **The inputs, before the document is built out of them**
        // (`BT_PREVIEW_TRACE`): a body laid out at a scale of zero or into a
        // rectangle the solver has not answered for yet is a body whose every
        // paragraph is a box no clip can keep, and by the time the picture is on
        // screen the numbers that made it are gone.
        //
        // **`bytes` and `owed` are the two the 2026-08-23 report needed and did
        // not have.** `built … paragraphs=0` was saying two completely different
        // things with one number — "this document really is empty" and "the head
        // read never came home" — and it was the second, because the answer had
        // been taken off the shared channel by another window and dropped. The
        // buffer's own two facts separate them at the station that already runs:
        // how much text it is holding, and whether it is still owed a read.
        preview_trace::emit(preview_trace::global(), || {
            let scroll = self
                .preview_pane(surface)
                .map_or([0.0, 0.0], |pane| pane.scroll);
            let buffer = self.preview_buffer_on(surface);
            let bytes = buffer.map_or(0, |buffer| buffer.content.as_deref().map_or(0, str::len));
            let owed = buffer.is_some_and(preview::PreviewBuffer::awaiting_head_read);
            format!(
                "build {surface:?} scale={scale} body=[{},{},{},{}] scroll=[{},{}] bytes={bytes} owed={}",
                body[0],
                body[1],
                body[2],
                body[3],
                scroll[0],
                scroll[1],
                u8::from(owed)
            )
        });
        self.rebuild_preview_document(surface, body, scale);
        if self.preview_buffer_on(surface).is_none() {
            preview_trace::emit(preview_trace::global(), || {
                format!("built {surface:?} leave=no-buffer")
            });
            return None;
        }
        let palette = bt_render::chrome_palette();
        let pane = self.preview_pane(surface)?;
        let scroll = pane.scroll;
        let advance = pane.mono_advance;
        let lit = self.preview_block_lit(surface);
        let (rows_height, columns) = self.preview_content_extent(surface, scale);
        let mut sites = Vec::new();
        let mut math_sites: Vec<PreviewMathSite> = Vec::new();
        let mut text_sites: Vec<PreviewTextSite> = Vec::new();
        // Before the document is borrowed, because this asks the pane and the
        // buffer the same questions the body below is about to hold.
        let caret_paint = self.preview_markdown_caret(surface, scale);
        let mut built = match &self.preview_pane(surface)?.doc {
            PreviewDocument::Text {
                lines,
                wrap,
                highlight,
            } => {
                let geometry = self.preview_doc_text_geometry(
                    surface,
                    body,
                    scale,
                    rows_height,
                    columns,
                    advance,
                );
                let edit = self.preview_edit_paint(surface, &geometry, wrap, scale);
                build_preview_text_body(
                    &geometry,
                    lines,
                    wrap,
                    highlight,
                    advance,
                    edit.as_ref(),
                    &palette,
                )
            }
            PreviewDocument::Diff(rows) => build_preview_diff_body(
                &seats::preview_mono_geometry(
                    body,
                    seats::preview_diff_metrics(scale),
                    rows_height,
                    columns,
                    advance,
                    scroll,
                ),
                rows,
                &palette,
            ),
            PreviewDocument::Table { rows, column_cells } => build_preview_table_body(
                &seats::preview_table_geometry(
                    body,
                    column_cells,
                    rows.len(),
                    advance,
                    scale,
                    scroll,
                ),
                rows,
                &palette,
            ),
            PreviewDocument::Markdown {
                blocks,
                source,
                intrinsic,
                layout,
                math,
                pictures,
                ranges: _,
                // The painter walks pieces it has boxes for; where those pieces
                // came from is the press's question and the highlight's, both of
                // which are answered off the pane rather than in here.
                maps: _,
                wrap: _,
            } => {
                let rendered = build_preview_markdown_body(
                    body,
                    seats::preview_markdown_metrics(scale),
                    scroll,
                    BlockScrollPaint {
                        offsets: &self.preview_pane(surface)?.md_block_scroll,
                        lit,
                        scale,
                    },
                    MarkdownPage {
                        blocks,
                        intrinsic,
                        layout,
                        live: MarkdownLive {
                            source: source.as_deref(),
                            caret: caret_paint.as_ref(),
                        },
                    },
                    &palette,
                    PageArt {
                        math,
                        pictures,
                        theme: bt_render::current_theme(),
                    },
                );
                sites = rendered.links;
                math_sites = rendered.math;
                text_sites = rendered.text;
                rendered.body
            }
            PreviewDocument::Empty => bt_render::PreviewBody {
                clip: body,
                quads: Vec::new(),
                paragraphs: Vec::new(),
                blocks: Vec::new(),
                rasters: Vec::new(),
            },
        };
        // **Every notice, one home** (user ruling, 2026-08-15).
        //
        // Nothing about the file is drawn *into* the document any more. There
        // were three homes and this was two of them — a 28px read-only bar with
        // height the body was shortened for, and a floating strip laid over the
        // body's last rows for a save's refusals — and both are gone. Every
        // sentence this window says *about* a buffer now hangs on the right hand
        // of the one strip that was always going to be there, the path foot
        // ([`seats::dress_foot`]); the flashed word keeps its own place at that
        // strip's left, and steps in front of the phrase while it stands.
        //
        // What is left here is the document and nothing else, which is why the
        // glance card — whose foot is a fixed sentence rather than a path, and
        // which used to be excluded from this whole paragraph by name — now
        // takes exactly the same path through this function as the other two,
        // and hangs the same phrase on the same side of its own foot.
        // The links after the body and never before it, because measuring one
        // asks the shaper where a paragraph landed and that answer is only true
        // of the body as it now stands.
        // The formulas ride with the links, and for the identical reason: both
        // are answers about where a run of an already-built body came to rest,
        // and both stop being true the moment the body is rebuilt.
        place_preview_math(
            &mut self.app.gpu,
            &mut self.window.renderer,
            &mut built,
            &math_sites,
        );
        // **The links are measured here, before the fills that stand under
        // them** (§7.1.3k ⑬). They were measured at the foot of this function
        // until a chip needed a ground: the *paragraphs* are final from the
        // builder on and nothing below moves one, so a box measured here is the
        // same box measured there — but a pill painted after the selection
        // bands would be a pill painted **over** them, and a selected badge
        // would lose its highlight to its own ground.
        let links = measure_preview_links(
            &mut self.app.gpu,
            &mut self.window.renderer,
            &built,
            &sites,
            scale,
        );
        // Every chip's pill, in the fence's own ground so that a chip reads as
        // the card it replaced, composited into the body it stands on —
        // `bt_render::rounded_preview_fill` says why a rounded fill in a body is
        // spent as colour rather than as alpha.
        let chip_grounds: Vec<bt_render::PreviewQuad> = links
            .iter()
            .filter(|link| link.chip.is_some())
            .flat_map(|link| {
                bt_render::rounded_preview_fill(
                    link.rect,
                    MARKDOWN_CHIP_RADIUS_LOGICAL_PX * scale,
                    palette.preview_code_ground,
                    palette.seat_body,
                )
            })
            .collect();
        built.quads.extend(chip_grounds);
        // **The selection under the rule a hovered link wears**: the bands are
        // fills and go out in the pass that draws every fill, and the one thing
        // that must be over them is that rule — which is struck last, below.
        //
        // Pushed here rather than inside the builder for the reason the links
        // are measured here: which glass a range of a document covers is a
        // question only the shaper answers, and the builder holds no shaper.
        let text_boxes = preview_text_boxes(&built, &text_sites);
        // The range is read while the pane is borrowed and the shaper is asked
        // after: `range_in` builds only the two blocks the ends stand in, which
        // is what makes a highlight on a 64KB page cost two blocks a frame
        // rather than every string in the file.
        // **The caret's selection first, because on a page that has one it is
        // the only one** (T5 ④, research §10 Q3). It arrives here as a range of
        // *file* bytes and leaves as the two places a highlight is drawn
        // between, mapped by T6's provenance (§7.1.3r) — so a selection begun in
        // the source block and dragged into the prose under it is one band
        // across both, each face drawing its own half in its own arithmetic.
        // The source block's half is the painter's
        // ([`push_markdown_source_block`], which cuts the same range against the
        // block it is drawing); this is every other block's.
        let range = self.preview_caret_selection_places(surface).or_else(|| {
            self.preview_pane(surface).and_then(|pane| {
                let selection = pane.md_select?;
                let PreviewDocument::Markdown { blocks, .. } = &pane.doc else {
                    return None;
                };
                Some(selection.range_in(blocks))
            })
        });
        if let Some((start, end)) = range {
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let bands =
                preview_selection_bands(&text_boxes, start, end, &mut |paragraph, range| {
                    renderer.measure_preview_highlight(gpu, paragraph, range)
                });
            built.quads.extend(bands.into_iter().map(|rect| {
                bt_render::PreviewQuad {
                    rect,
                    // **The terminal's own selection blue** — the fill
                    // `preview_selection` was minted to lend the quick edit, and
                    // lent to a second surface here for its own stated reason:
                    // two blues on one screen for one idea would be the window
                    // disagreeing with itself.
                    color: palette.preview_selection,
                }
            }));
        }
        self.preview_pane_mut(surface).md_text = text_boxes;
        // **The prose block's own geometry, and everything struck in it**
        // (§7.1.3w). Here rather than in the builder for the bands' own reason,
        // one paragraph up: which glass a byte of a proportional row covers is a
        // question only the shaper answers, and the builder holds no shaper.
        // Asked once and read five times — by the bar below, by the bands below
        // it, and between frames by the press, the IME and the arrow keys.
        let prose = self.preview_prose_geometry(surface, scale, caret_paint.as_ref());
        if let (Some(prose), Some(caret)) = (&prose, &caret_paint) {
            // The selection is the file's and this block is a window onto it,
            // exactly as [`push_markdown_source_block`] cuts the same range
            // against the monospace face.
            built.quads.extend(
                prose
                    .bands(&caret.selection)
                    .into_iter()
                    .filter_map(|band| bt_render::crop_to(band, built.clip))
                    .map(|rect| bt_render::PreviewQuad {
                        rect,
                        color: palette.preview_selection,
                    }),
            );
            // **A composition stands between the caret's byte and the caret**,
            // which is the text face's own sentence ([`build_preview_text_body`])
            // and the monospace block's ([`push_markdown_source_block`]), said
            // in seams because this face has no cells: the letters were set into
            // the paragraph above, the rule under them says they are not in the
            // file yet, and the caret is inside them where the input method put
            // it rather than at the byte they were typed in front of.
            let composed = caret.lit.then_some(prose.composition.as_ref()).flatten();
            if let Some(composition) = composed {
                built.quads.extend(
                    composition
                        .rows
                        .iter()
                        .filter_map(|row| {
                            bt_render::crop_to(
                                [row[0], row[3] - caret.caret_width, row[2], row[3]],
                                built.clip,
                            )
                        })
                        .map(|rect| bt_render::PreviewQuad {
                            rect,
                            color: palette.preview_body_text,
                        }),
                );
            }
            let bar = match composed {
                Some(composition) => composition.caret,
                None => match caret.seat {
                    MarkdownCaretSeat::Prose(offset) => prose.caret(offset),
                    MarkdownCaretSeat::Source(..) | MarkdownCaretSeat::Gap { .. } => None,
                },
            };
            if caret.lit
                && let Some([x, top, _, bottom]) = bar
                && let Some(rect) =
                    bt_render::crop_to([x, top, x + caret.caret_width, bottom], built.clip)
            {
                built.quads.push(bt_render::PreviewQuad {
                    rect,
                    color: palette.preview_caret,
                });
            }
        }
        self.preview_pane_mut(surface).md_prose = prose;
        // The hover's rule is drawn from the boxes measured above, so the line
        // under a link cannot be anywhere but under it — and it is struck here,
        // last of the fills, so that nothing else is laid over it.
        if let Some((hovered_surface, hovered)) = self.preview_link_hover.as_ref()
            && *hovered_surface == surface
            && links.iter().any(|link| link.rect == hovered.rect)
        {
            let rect = hovered.rect;
            let thickness = (scale).round().max(1.0);
            built.quads.push(bt_render::PreviewQuad {
                rect: [rect[0], rect[3] - thickness, rect[2], rect[3]],
                color: palette.accent,
            });
        }
        self.preview_pane_mut(surface).links = links;
        preview_trace::emit(preview_trace::global(), || {
            format!(
                "built {surface:?} paragraphs={} quads={} blocks={}",
                built.paragraph_count(),
                built.quad_count(),
                built.blocks.len()
            )
        });
        Some(built)
    }

    /// The caret and the selection, in the columns the painter draws them in.
    ///
    /// **Only the visible lines**, on the same principle the rest of the body is
    /// built on: a selection over a 64KB file covers two thousand rows and a
    /// pane shows forty, and a band per row would put the file's size into the
    /// frame's cost.
    /// **The caret on a rendered markdown page, ready to be drawn** (§7.1.3q) —
    /// [`Self::preview_edit_paint`]'s twin, one face over.
    ///
    /// Every number in it is a paint-time number, which is the other half of
    /// [`PreviewDocumentKey::source`]'s argument: the document knows *which*
    /// block is source and nothing more, so a caret walking along one paragraph
    /// costs this function per frame and never a re-layout.
    ///
    /// **The seat is re-asked here and matched against the document's**, not
    /// taken from it. The two can disagree for exactly one frame — a keystroke
    /// moved the caret into another block and the parse that follows it has not
    /// been laid out yet — and drawing this frame's caret into the last frame's
    /// source block would put it in the wrong paragraph. When they disagree the
    /// caret is simply not drawn for that frame, which is a frame, and the next
    /// one has both.
    /// **What a caret's selection covers, in the page's own places** (T5 ④,
    /// §7.1.3t).
    ///
    /// The inverse of the press: a press asks
    /// [`preview_provenance::file_offset_of`] which byte of the file a place is,
    /// and the highlight asks [`preview_provenance::place_of`] where in the page
    /// a byte of the file is drawn. Both ends are rounded by that module's own
    /// rule when they name a byte the page does not draw — a heading's hashes, a
    /// quote's `>` — which is what keeps a band a band rather than making it
    /// vanish at the marks inside it.
    ///
    /// `None` for a page with no caret in it and for an empty selection, which
    /// is the same `None` the piece model gives and lets the caller fall through
    /// to it.
    fn preview_caret_selection_places(
        &self,
        surface: PreviewSurface,
    ) -> Option<(preview_select::Place, preview_select::Place)> {
        let caret = self.preview_live_caret(surface)?;
        let range = caret.range();
        if range.is_empty() {
            return None;
        }
        let PreviewDocument::Markdown {
            blocks,
            ranges,
            maps,
            ..
        } = &self.preview_pane(surface)?.doc
        else {
            return None;
        };
        let start = preview_provenance::place_of(range.start, blocks, ranges, maps)?;
        let end = preview_provenance::place_of(range.end, blocks, ranges, maps)?;
        Some((start, end))
    }

    /// **The caret's prose block, as the shaper that drew it laid it out**
    /// (§7.1.3w) — the one geometry the bar, the band, the candidate box, the
    /// press and the arrow keys all read.
    ///
    /// The paragraphs are built by the very function the painter builds them
    /// with ([`markdown_prose_paragraphs`]) out of the very block the document
    /// is carrying, so what is measured here is what is on the glass; the offsets
    /// come back as the **file's** own bytes, because every byte this block draws
    /// is a byte of the file at its own offset.
    ///
    /// Every row, not only the ones on screen: a page scrolled so that half the
    /// block is above it still has to answer Up and Down about the rows that are
    /// not showing, and the count is the block's lines rather than the document's.
    pub(crate) fn preview_prose_geometry(
        &mut self,
        surface: PreviewSurface,
        scale: f32,
        caret: Option<&MarkdownCaretPaint>,
    ) -> Option<preview_live::ProseRows> {
        let palette = bt_render::chrome_palette();
        // The document's borrow ends with this expression, before the shaper's
        // begins.
        let (index, paragraphs) = {
            match self.markdown_caret_box(surface, scale) {
                Some((box_of_block, block, placed)) if block.prose().is_some() => {
                    let body = self.preview_surface_body_rect(surface, scale)?;
                    if box_of_block[3] <= body[1] || box_of_block[1] >= body[3] {
                        return None;
                    }
                    let prose = block.prose()?;
                    (
                        Some(prose.index),
                        markdown_prose_paragraphs(
                            prose,
                            box_of_block,
                            &placed.rows,
                            markdown_prose_composition(caret),
                            &palette,
                        ),
                    )
                }
                // **The gap's empty line is measured too, and only while
                // something is being composed on it** (§7.1.3q). It is no block
                // of the document, so there is nothing to splice into and
                // nothing to read back: the whole paragraph is the composition,
                // and what the shaper is being asked is where the input method's
                // own caret stands inside letters that are not in the file.
                _ => (None, self.markdown_gap_paragraphs(surface, scale, caret)?),
            }
        };
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut rows = Vec::new();
        let mut composition = preview_live::ProseComposition::default();
        for line in &paragraphs {
            for row in renderer.measure_preview_rows(gpu, &line.paragraph) {
                let cut = preview_live::split_prose_row(
                    row.top,
                    row.height,
                    &row.seams
                        .iter()
                        .map(|seam| preview_live::ProseSeam {
                            offset: seam.offset,
                            x: seam.x,
                        })
                        .collect::<Vec<_>>(),
                    line.start,
                    line.splice,
                );
                rows.push(cut.row);
                composition.rows.extend(cut.composition);
                composition.caret = composition.caret.or(cut.caret);
            }
        }
        Some(preview_live::ProseRows {
            index,
            rows,
            composition: (!composition.is_empty()).then_some(composition),
        })
    }

    fn preview_markdown_caret(
        &self,
        surface: PreviewSurface,
        scale: f32,
    ) -> Option<MarkdownCaretPaint> {
        let caret = self.preview_live_caret(surface)?;
        let content = self.preview_buffer_on(surface)?.content.as_deref()?;
        let PreviewDocument::Markdown { ranges, source, .. } = &self.preview_pane(surface)?.doc
        else {
            return None;
        };
        let seat = match preview_live::caret_seat(content, ranges, caret.caret) {
            preview_live::CaretSeat::Block(index) => {
                let source = source.as_deref().filter(|block| block.index() == index)?;
                match source {
                    MarkdownCaretBlock::Mono(_) => {
                        let (line, column) =
                            preview_live::place_in_block(content, ranges, index, caret.caret)?;
                        MarkdownCaretSeat::Source(line, column)
                    }
                    MarkdownCaretBlock::Prose(_) => MarkdownCaretSeat::Prose(caret.caret),
                }
            }
            // **A gap takes the prose face too** (§7.1.3w): it is an empty line
            // of a document whose prose is set in the body face, and a caret a
            // monospace line tall standing between two paragraphs would be a
            // caret announcing a face nothing on the page is set in.
            preview_live::CaretSeat::Gap { after } => MarkdownCaretSeat::Gap {
                after,
                line_height: seats::preview_markdown_metrics(scale).line_height,
            },
        };
        Some(MarkdownCaretPaint {
            seat,
            // The caret belongs to the focus and the selection does not — a body
            // you have clicked away from keeps what it had marked, greyed, and
            // has no caret because nothing is going to land there.
            lit: self.preview_edit_focus() == Some(surface),
            selection: caret.range(),
            caret_width: (bt_render::CURSOR_BAR_WIDTH_LOGICAL_PX * scale)
                .round()
                .max(1.0),
            preedit: self
                .preview_preedit(surface)
                .map(|preedit| MarkdownPreedit {
                    text: preedit.text.clone(),
                    caret_byte: preedit.cursor_byte.unwrap_or(preedit.text.len()),
                }),
        })
    }

    /// **The composition this page is entitled to draw** —
    /// [`Self::shell_preedit`]'s twin, and the same single ownership said one
    /// surface over.
    ///
    /// One `preedit` field for the window because there is one composition, so
    /// the letters belong wherever the keyboard is: a composition made at a
    /// shell prompt must not also be overlaid on a document behind it, and one
    /// made in a page must not be drawn on a second page in the other pane.
    fn preview_preedit(&self, surface: PreviewSurface) -> Option<&Preedit> {
        (self.preview_edit_focus() == Some(surface)
            && matches!(ime_owner(self.keyboard_owner()), ImeOwner::Preview))
        .then_some(self.window.preedit.as_ref())
        .flatten()
    }

    fn preview_edit_paint(
        &self,
        surface: PreviewSurface,
        geometry: &seats::PreviewMonoGeometry,
        wrap: &preview_edit::WrapLayout,
        scale: f32,
    ) -> Option<PreviewEditPaint> {
        if !self.preview_is_editable(surface) {
            return None;
        }
        let buffer = self.preview_buffer_on(surface)?;
        let content = buffer.content.as_deref()?;
        let starts = self.preview_buffer_on(surface)?.line_starts();
        let range = visible_range(
            geometry.line_rect(0)[1],
            geometry.line_height,
            wrap.rows(),
            geometry.viewport,
        );
        let selection = self.preview_pane(surface)?.caret.range();
        // **Cut per visual row, not per line.** A selection is a range of the
        // *line*, and a wrapped line is several rows: a band drawn once for the
        // whole line would start on the first row and run off its right edge
        // instead of turning the corner with the text it is under.
        let bands = preview_edit_bands(content, starts, &selection, wrap, range);
        // The caret belongs to the *focus*, not to the buffer: a body you have
        // clicked away from keeps its selection, greyed, the way a text field
        // does, but it has no caret because nothing is going to land there.
        let caret = (self.preview_edit_focus() == Some(surface))
            .then(|| self.preview_caret_position(surface))
            .flatten()
            .map(|(line, column)| preview_caret_row(wrap, line, column));
        // The window's one composition, drawn here because this is where the
        // keyboard is — see [`Self::shell_preedit`] for the other half of that
        // single ownership.
        let preedit = caret
            .zip(self.window.preedit.as_ref())
            .map(|((row, column), preedit)| PreviewPreedit {
                row,
                column,
                columns: preview_edit::line_columns(&preedit.text),
                caret_columns: preedit.cursor_byte.map_or_else(
                    || preview_edit::line_columns(&preedit.text),
                    |byte| preview_edit::column_of(&preedit.text, byte),
                ),
                text: preedit.text.clone(),
            });
        Some(PreviewEditPaint {
            bands,
            caret,
            caret_width: (bt_render::CURSOR_BAR_WIDTH_LOGICAL_PX * scale)
                .round()
                .max(1.0),
            preedit,
        })
    }

    /// **What the picture on this surface is** — `PNG · 670 KB`,
    /// `6000 × 4000 · shown at 41%` — in the row above it (owner's ruling
    /// 2026-09-12).
    ///
    /// It was `.pv-meta`, a line under the picture (mock-up 4955), until the
    /// ruling moved it: it is a standing fact about the file, it belongs with
    /// the other one — the padlock — at the right end of the path row, and a
    /// sentence pinned under a photograph was the last thing between it and the
    /// bottom of the pane.
    ///
    /// Everything it can say and nothing it cannot: a field the window does not
    /// know yet is left out rather than printed as a placeholder, so the line
    /// grows as the decoder and the worker answer instead of flickering through
    /// three shapes.
    ///
    /// **A recording's sentence is not here** — see [`Self::video_meta_sentence`],
    /// which puts it at the right end of the player's own bar, because that is
    /// where a recording's facts belong once the bar is drawn on the picture.
    fn preview_meta_sentence(&self, surface: PreviewSurface, scale: f32) -> Option<String> {
        let image = self.preview_pane(surface)?.image.as_ref()?;
        if preview::path_names_a_video(&image.path) {
            return None;
        }
        let extension = image
            .path
            .extension()
            .map(|ext| ext.to_string_lossy().to_uppercase())
            .filter(|ext| !ext.is_empty());
        // The zoom is said only once the decode has answered: a percentage is a
        // multiple of native pixels, and there is no such thing to be a multiple
        // of until somebody has opened the file.
        let body = self.preview_surface_body_rect(surface, scale)?;
        let zoom = self.preview_image_zoom(surface);
        let caption = image
            .native
            .map(|(width, height)| image_zoom_caption(body, [width, height], zoom));
        // **The file's own size first, and what is on the glass beside it**
        // (owner's ruling 2026-09-12). A picture the decoder reduced to fit the
        // pixel budget has two true sizes — see
        // [`PreviewImageState::stated_size`] — and the reader is owed the one
        // about the file, because that is the one that travels with a copy of it.
        let (stated, shown) = match image.stated_size {
            Some(size) => (Some(size), image.native.filter(|native| *native != size)),
            None => (image.native, None),
        };
        image_meta_sentence(
            stated,
            shown,
            extension.as_deref(),
            image.bytes,
            caption.as_deref(),
        )
    }

    /// **What the recording on this surface is** — `MP4 · 38 MB`, at the right
    /// end of its own bar (owner's ruling 2026-09-12; §7.44).
    ///
    /// The same facts the card says about the same file, built by the same
    /// function so the two surfaces cannot come to disagree about one recording
    /// (user ruling 2026-08-27; §7.23) — and **joined into one line rather than
    /// stacked into two**, which is not a compromise: the card stacks only
    /// because a card is 280 pixels wide.
    ///
    /// The zoom is not among them because there is no zoom: a video's frame
    /// stands at Fit and the wheel over it moves nothing
    /// ([`Self::picture_takes_zoom`], where the whole argument is written).
    ///
    /// It stands on the bar and no longer under the picture, which is the
    /// ruling's own sentence about this surface: the bar is flush with the
    /// bottom of the stage now, so a line under the frame would either be behind
    /// the bar or be the band the ruling retired.
    fn video_meta_sentence(&self, surface: PreviewSurface) -> Option<String> {
        // **The recording this surface is playing, and only then the still it is
        // showing.** A pane that has started playing has no `image` left — the
        // decoded first frame comes off the glass the moment the engine has one
        // (`refit_preview_picture`) — so a sentence asked of the still alone
        // would be a bar that says what the file is until you press play and
        // then stops.
        let path = match self.video_playing_on(surface) {
            Some(path) => path.to_path_buf(),
            None => self.preview_pane(surface)?.image.as_ref()?.path.clone(),
        };
        if !preview::path_names_a_video(&path) {
            return None;
        }
        let facts = self.video_facts_of(&path);
        let extension = path.extension().and_then(std::ffi::OsStr::to_str);
        let sentence = preview::video_fact_lines(extension, facts)
            .into_iter()
            .flatten()
            .collect::<Vec<String>>()
            .join(" \u{b7} ");
        (!sentence.is_empty()).then_some(sentence)
    }

    /// How one surface is looking at its picture (ticket #60).
    ///
    /// The glance card is answered `Fit` whatever it happens to be storing —
    /// [`surface_takes_image_zoom`] is the whole of that rule and this is one of
    /// its two readers, the other being the door below that declines to write.
    pub(crate) fn preview_image_zoom(&self, surface: PreviewSurface) -> ImageZoom {
        if !self.picture_takes_zoom(surface) {
            return ImageZoom::FIT;
        }
        self.preview_pane(surface)
            .map_or(ImageZoom::FIT, |pane| pane.zoom)
    }

    /// **Whether the picture on this surface may be zoomed** — the surface's own
    /// rule ([`surface_takes_image_zoom`]) and the *content's*.
    ///
    /// **A video's frame does not zoom** (user ruling 2026-08-27; §7.23), and the
    /// reason is the sentence under it rather than the picture itself. A
    /// percentage on this window means "how many of the file's own pixels am I
    /// seeing", and a frame has no such number to be a multiple of: it was
    /// decoded into [`VIDEO_FRAME_FIT_PX`], so `100%` over a 4K capture would be
    /// half of it — a magnification of the *thumbnail* printed beside the
    /// recording's real resolution, two numbers about size meaning different
    /// things. And a gesture that changes what a reader is looking at without
    /// telling them what it changed it to is the one thing `page_foot_flash`
    /// exists to have ended.
    ///
    /// So the frame is a face and not a viewer: it stands at Fit, the wheel over
    /// it spends a notch on nothing, and the two fact lines are the whole of what
    /// the pane says. Zooming a video is a thing to want *while it plays*, which
    /// is §7.23 ④'s slice and will bring its own readout.
    ///
    /// Said here rather than at each of the five gestures for the reason its
    /// surface half is said once: one of them would eventually be written
    /// without it.
    fn picture_takes_zoom(&self, surface: PreviewSurface) -> bool {
        surface_takes_image_zoom(surface)
            && !self
                .preview_picture(surface)
                .is_some_and(|picture| preview::path_names_a_video(&picture.path))
    }

    /// Put a new zoom on a surface and repaint if it moved.
    ///
    /// Returns whether anything changed, so the gestures above can stay silent
    /// on a notch spent at the end of the road — a wheel at 800% that
    /// republished the frame would be a repaint per notch for no pixels.
    ///
    /// **A scale that moved is a picture still settling** ([`Self::defer_preview_resample`]'s
    /// boundary, reached by its second gesture). The exact-size raster is a
    /// question about *where the picture came to rest*, and a wheel is a run of
    /// notches, so asking it once per notch asks it about sizes the hand is
    /// still travelling through. Measured on a 4000×3000 PNG, ten detents of
    /// zoom put seven questions to the resample lane, of which the lane had time
    /// to answer three — each a Lanczos3 pass of 240–480ms over a size the
    /// picture had already left. The two that landed early were not sharper
    /// pictures arriving, they were *older* pictures arriving: the reader saw
    /// the raster jump twice on its way to the one it should have had.
    ///
    /// So the scale defers exactly as a resize does, through the very same
    /// deadline, and the one pass that runs is the one the gesture ended on.
    /// **The pan does not**, and that asymmetry is the whole of the rule: a pan
    /// moves the picture across its body without changing how many pixels of it
    /// are shown, so the raster it is already holding is still the exact right
    /// one and there is nothing to settle.
    pub(crate) fn set_preview_image_zoom(
        &mut self,
        surface: PreviewSurface,
        zoom: ImageZoom,
    ) -> Result<bool> {
        let held = self.preview_image_zoom(surface);
        if !self.picture_takes_zoom(surface) || held == zoom {
            return Ok(false);
        }
        if image_zoom_settles(held, zoom) {
            let now = Instant::now();
            if let Some(picture) = self.preview_picture_mut(surface) {
                picture.defer_scale_settle(now);
            }
        }
        self.preview_pane_mut(surface).zoom = zoom;
        // **What one notch costs, split by the layer that spends it.** The zoom
        // gesture is the one place three whole-window rebuilds are run back to
        // back off a single input event, so "zooming is a little sticky" has
        // three candidate authors and no way to tell them apart from the
        // outside. Printed under the same switch every other latency on this
        // window is printed under.
        let started = self.app.trace_perf.then(Instant::now);
        self.refresh_preview_for_layout();
        let laid_out = started.map(|_| Instant::now());
        self.refresh_chrome();
        let chromed = started.map(|_| Instant::now());
        // The zoom is the only thing that decides whether the picture can be
        // carried, so a notch is the one event that changes the hand's shape
        // under a pointer that never moved — the chrome hover, which is what
        // ordinarily reapplies it, has nothing new to notice here.
        self.apply_pointer_cursor();
        self.present_chrome_change()?;
        if let (Some(started), Some(laid_out), Some(chromed)) = (started, laid_out, chromed) {
            let scale = self
                .preview_image_geometry(surface)
                .map_or(f32::NAN, |(body, image_px)| {
                    image_zoom_scale(body, image_px, zoom)
                });
            trace_sink::stderr_line(format!(
                "BT_PERF_TRACE image_zoom scale={scale:.4} layout_us={} chrome_us={} present_us={} total_us={}",
                (laid_out - started).as_micros(),
                (chromed - laid_out).as_micros(),
                chromed.elapsed().as_micros(),
                started.elapsed().as_micros(),
            ));
        }
        Ok(true)
    }

    /// The picture one surface is showing and the body it is shown in — the pair
    /// every zoom gesture needs and none of them may invent for itself.
    ///
    /// `None` when the surface is showing a document, or a picture whose decode
    /// has not landed: a zoom about a size nobody knows yet would be arithmetic
    /// on a guess, and the gesture is better spent on nothing than on that.
    pub(crate) fn preview_image_geometry(
        &self,
        surface: PreviewSurface,
    ) -> Option<([f32; 4], [u32; 2])> {
        let (width, height) = self.preview_pane(surface)?.image.as_ref()?.native?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let body = self.preview_surface_body_rect(surface, scale)?;
        (width > 0 && height > 0).then_some((body, [width, height]))
    }

    /// **Every picture on screen, and nothing that is not one.**
    ///
    /// **One rule for both hosts, since §7.1.6k⁷: a surface holding a picture is
    /// a picture on screen.** It used to be two rules, and the asymmetry was the
    /// renderer's and not the reader's — a *float's* pixels ride its own overlay
    /// layer ([`Self::preview_float_layer`]), so every float was here, while a
    /// *seat's* went down `bt_render`'s single `set_preview_image` slot, so at
    /// most one seat was, and [`PreviewPane::image`] on every other pane meant a
    /// head, a foot and a meta line over nothing at all. That slot is a list now
    /// ([`bt_render::WindowRenderer::set_preview_images`]) and the election it
    /// forced is gone with it.
    ///
    /// Every float, not only this tab's, for [`Self::preview_surfaces`]' reason:
    /// a window torn out of another tab is still standing, and a picture that
    /// stopped being refit the moment you looked somewhere else would freeze at
    /// whatever size it was last seen at.
    fn preview_picture_hosts(&self) -> Vec<PreviewSurface> {
        // The seat half is the tab's own rule, asked of the tab
        // ([`TabState::seat_pictures`]), because that is the answer
        // [`Self::pane_draws`] has to agree with frame by frame and two
        // spellings of it is how the last two defects in this family began.
        let mut hosts = self.seat_pictures();
        hosts.extend(
            self.preview_surfaces()
                .into_iter()
                // The glance card mirrors a picture through its own path
                // ([`Self::file_peek_picture`]) and holds no
                // `PreviewImageState` to refit.
                .filter(|surface| matches!(surface, PreviewSurface::Float(_)))
                .filter(|surface| {
                    self.preview_pane(*surface)
                        .is_some_and(|pane| pane.image.is_some())
                }),
        );
        hosts
    }

    /// The picture one surface is showing, if it is showing one.
    pub(crate) fn preview_picture(&self, surface: PreviewSurface) -> Option<&PreviewImageState> {
        self.preview_pane(surface)?.image.as_ref()
    }

    /// The same, mutably.
    fn preview_picture_mut(&mut self, surface: PreviewSurface) -> Option<&mut PreviewImageState> {
        self.preview_pane_mut(surface).image.as_mut()
    }

    /// **This surface's picture is not on screen this frame.**
    ///
    /// One door for every refusal below — the decode is still out, the body has
    /// no extent, the host has no rectangle to give — because the two hosts stop
    /// drawing in two different ways and no refusal should have to know which of
    /// them it is talking to. Taking `drawn` away is the whole of it: it is the
    /// sentence the float layer reads, and it is what
    /// [`Self::refit_preview_picture`] answers `None` with, so this surface is
    /// simply not among the pictures the seat pass is handed this frame.
    ///
    /// The pixels themselves are **kept**. A refusal is about this frame, and a
    /// raster thrown away here is a raster the worker is asked for again the
    /// moment the refusal lifts — which is exactly the resample storm R2 exists
    /// to forbid.
    fn hide_preview_picture(&mut self, surface: PreviewSurface) {
        if let Some(picture) = self.preview_picture_mut(surface) {
            picture.drawn = None;
        }
    }

    /// The earliest settle boundary any picture on screen is waiting on.
    ///
    /// Plural for [`Self::preview_picture_hosts`]' reason: a window resize defers
    /// every picture's exact-size resample at once, and a wake scheduled off the
    /// first of them only would leave the others soft until something else
    /// happened to redraw. A zoom defers one picture — the one under the wheel —
    /// but it is read from the same place, because the question a wake answers
    /// is "is any picture owed a raster yet", not "which gesture owes it".
    pub(crate) fn preview_resample_deadline(&self) -> Option<Instant> {
        self.preview_picture_hosts()
            .into_iter()
            .filter_map(|surface| self.preview_picture(surface)?.scale_settle_deadline)
            // **And the pictures a markdown page carries** (§7.1.3k), which wait
            // on the same boundary for the same reason and are read from the
            // same place: what a wake answers is "is any picture owed a raster
            // yet", not "which surface owes it".
            .chain(self.window.markdown_pictures.settle_deadline)
            .min()
    }

    pub(crate) fn defer_preview_resample(&mut self, observed_at: Instant) {
        for surface in self.preview_picture_hosts() {
            if let Some(picture) = self.preview_picture_mut(surface) {
                picture.defer_scale_settle(observed_at);
            }
        }
        if self.window.markdown_pictures.settle_deadline.is_some() {
            self.window.markdown_pictures.settle_deadline = Some(observed_at + WINDOW_RESIZE_QUIET);
        }
    }

    pub(crate) fn finish_preview_scale_if_quiet(&mut self, now: Instant) -> Result<()> {
        let mut due = false;
        for surface in self.preview_picture_hosts() {
            if let Some(picture) = self.preview_picture_mut(surface) {
                due |= picture.finish_scale_settle_if_quiet(now);
            }
        }
        if self
            .window
            .markdown_pictures
            .settle_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            due |= self.send_owed_markdown_rasters();
        }
        if due {
            self.refresh_preview_for_layout();
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Refit every picture on screen to the body its host gives it this frame.
    /// Decodes stay on the decoration worker and Lanczos3 runs on its independent
    /// scale lane; this method only routes shared data.
    pub(crate) fn refresh_preview_for_layout(&mut self) {
        // **The playing recordings first**, because the picture lane below
        // refuses to fit a still on a surface that is playing one and the two
        // answers have to be about the same frame (route B slice ②; §7.44 ③).
        // On the frames where nothing is playing both of these are one empty
        // walk over an empty map.
        self.sweep_video_seats();
        self.refresh_video_layers();
        // The document lane's half of the same refit. `refresh_preview_body`
        // rebuilds the parsed document first, so the heal below is clamping
        // against the extent this frame actually has rather than the last one's
        // — which is what makes a pane grown taller give its scroll back.
        self.refresh_preview_body();
        // **Between the rebuild and the heal, and that is the whole of why it is
        // here** (§7.1.5j ⑨). A surface opened at a line cannot reach it until the
        // rows exist, because the answer is a *row* and a wrapped body puts line
        // 40 well below the fortieth of them; and it has to land before the heal,
        // so the offset it writes is clamped by the same authority every other
        // scroll on this surface is clamped by. It costs nothing on the frames
        // that owe nothing — the pending line is `None` and the walk stops there.
        let mut seated = false;
        for surface in self.preview_surfaces() {
            self.settle_preview_goto(surface);
            // **And the press that bought a whole file gets its caret** (T5 ①),
            // in the same walk and for the same reason: the answer needs a body
            // that was not there when the gesture was made, and the frame the
            // body lands on is this one.
            seated |= self.settle_preview_caret(surface);
        }
        if seated {
            // The body a moment ago was built for a page with no caret in it, so
            // the block the caret has just landed in has not been cut out of the
            // file yet. One rebuild, on the frames a press was waiting on.
            self.refresh_preview_body();
        }
        // **And the heal's own rebuild, for `settle_preview_goto`'s reason.** The
        // body a moment ago was built from the offset the heal has just replaced,
        // so on a frame that clamped anything the picture in the renderer's hands
        // is one nothing on this pane agrees with any more — a document scrolled
        // past its own end draws no rows at all. Only on the frames that moved
        // something, which are the frames a pane grew taller or a document grew
        // shorter and no others.
        if self.heal_preview_scroll() {
            self.refresh_preview_body();
        }
        // **The seat pass's pictures, all of them, rebuilt from the panes.** The
        // whole list every time for `set_preview_bodies`' reason — these
        // rectangles come out of the layout anyway — and it is what empties the
        // channel too: a pane that has stopped holding a picture stops
        // contributing one, so nothing has to remember to release anything
        // (§7.1.6k⁷).
        let mut pictures: Vec<PreviewImage> = Vec::new();
        for surface in self.preview_picture_hosts() {
            if let Some(picture) = self.refit_preview_picture(surface) {
                pictures.push(picture);
            }
        }
        self.window.renderer.set_preview_images(pictures);
    }

    /// **Every playing recording, as this frame's video layers** (user ruling
    /// 2026-08-28, route B slice ②; `docs/DESIGN.md` §7.44 ③).
    ///
    /// **One function for all three surfaces, and the whole list every time**,
    /// which is the shape `bt_render::WindowRenderer::set_video_layers` asks for
    /// and asks for on purpose: a key that stops appearing is a video that has
    /// stopped, and its texture goes back the same frame (§7.42 ⑥). There is no
    /// cheaper half to move separately, because the rectangles are recomputed
    /// from the layout anyway.
    ///
    /// Each surface contributes three things and they are genuinely different:
    ///
    /// * **Its box.** A pane's travels with its pane's tween and carries the
    ///   crop a FLIP needs — the same [`preview_image_placement`] the still is
    ///   fitted by, so the picture does not move when play is pressed. A
    ///   float's is its window's `content_body`, raised clear of the rounded
    ///   floor. A card's is the rounded window its still frame already stood in.
    /// * **Its ground.** `None` in a pane, where the letterbox bars are the
    ///   pane's own body colour, already painted underneath; `Some` in a float
    ///   and on a card, where nothing has painted the box and two bars of
    ///   nothing would be a hole in the window.
    /// * **Its stage.** Three heights in the renderer's pass, because a float's
    ///   face is opaque and a card stands over every seat — see
    ///   [`bt_render::VideoStage`].
    fn video_layers(&self, animations: &[DrawnAnimation]) -> Vec<bt_render::VideoLayer> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let now = Instant::now();
        let mut layers = Vec::new();
        for (surface, seat) in self.window.video.iter() {
            let Some(shape) = self.video_shape_of(surface, scale, now) else {
                continue;
            };
            layers.push(seat.layer(
                shape.box_,
                shape.clip,
                shape.ground,
                shape.radius_px,
                1.0,
                shape.stage,
            ));
        }
        // **And the pictures that move on their own** (user ruling 2026-08-28;
        // §7.44 ⑤). Down the same lane, fitted by the same rule, staged in the
        // same three places — which is the whole reason a `.gif` needed thirty
        // lines of decoding and no second upload path at all.
        //
        // Which of them are on the glass is [`Self::drawn_animations`]'s answer
        // and not a second walk with the same conditions written out again: the
        // list the renderer is handed and the list the clock advances have to be
        // the same list, and B8 is what it costs when they are two.
        for drawn in animations {
            let Some(AnimationEntry::Ready { animation, .. }) =
                self.window.animations.get(&drawn.key)
            else {
                continue;
            };
            layers.push(bt_render::VideoLayer {
                // **The playback, not the surface** (adversarial review
                // 2026-09-11, B3). `gif:{surface:?}` named a box on the glass, so
                // a box handed a second file went on comparing the new
                // animation's generation against the old one's texture — and
                // rejected every upload until the new file's counter caught up,
                // which for a spinner switched in behind a long capture is a
                // quarter of an hour of the wrong picture. With the serial in the
                // name the second file is a texture the renderer has never seen,
                // and the first frame it hands over is the one that lands.
                //
                // The *window* is the other half of the identity and it is held
                // by the renderer's own map rather than spelled in here — see
                // `bt_render`'s `VideoTextureKey`. The serial is minted per
                // process, so this string is unique across windows either way;
                // what the map's key buys is that one window's frame cannot
                // release another window's textures.
                key: animation_layer_key(drawn.surface, drawn.serial),
                box_: drawn.shape.box_,
                clip: drawn.shape.clip,
                frame: Some(animation.upload()),
                ground: drawn.shape.ground,
                radius_px: drawn.shape.radius_px,
                opacity: 1.0,
                stage: drawn.shape.stage,
            });
        }
        layers
    }

    /// **Every animation this frame actually puts on the glass** — one entry per
    /// surface drawing one (adversarial review 2026-09-11, B3 and B8).
    ///
    /// The one authority for "drawn", and having one is the finding. The layer
    /// list was built from this walk, the clock was run over the whole cache and
    /// the refills were asked from a third walk over surfaces, so an animation
    /// could be advancing in a tab nobody was looking at while the only thing
    /// that would have refilled it was the fact that it was on screen. What
    /// follows from a single answer is short: what is drawn advances, what is
    /// drawn is refilled, what is drawn keeps the loop awake, and what is not
    /// drawn is a picture holding still.
    ///
    /// A surface that is playing a *recording* is skipped: it cannot be both,
    /// and the seat has already spoken for it.
    fn drawn_animations(&self) -> Vec<DrawnAnimation> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let now = Instant::now();
        let mut drawn = Vec::new();
        for surface in self.animated_surfaces() {
            if self.window.video.get(surface).is_some() {
                continue;
            }
            let Some(path) = self.animation_path_of(surface) else {
                continue;
            };
            let key = normalized_local_image_path_key(&path);
            let Some(AnimationEntry::Ready { serial, .. }) = self.window.animations.get(&key)
            else {
                continue;
            };
            let Some(shape) = self.video_shape_of(surface, scale, now) else {
                continue;
            };
            drawn.push(DrawnAnimation {
                key,
                path,
                serial: *serial,
                surface,
                shape,
            });
        }
        drawn
    }

    /// **Whether this surface's picture is one this window is running** — a
    /// `.gif` whose frames have arrived (§7.44 ⑤).
    ///
    /// The still and the animation are the same file at the same size in the
    /// same box, so exactly one of them may be on the glass: the video lane
    /// draws a whole pass before the picture lane, and a still left standing
    /// would be painted straight over the frame that moved. This is the same
    /// refusal `surface_is_playing_a_video` earns for a recording, said for the
    /// other kind of moving picture.
    pub(crate) fn animation_running_on(&self, surface: PreviewSurface) -> bool {
        self.animation_path_of(surface)
            .map(|path| normalized_local_image_path_key(&path))
            .and_then(|key| self.window.animations.get(&key))
            .is_some_and(|entry| matches!(entry, AnimationEntry::Ready { .. }))
    }

    /// The animated file one surface is showing, if it is showing one.
    fn animation_path_of(&self, surface: PreviewSurface) -> Option<PathBuf> {
        let path = match surface {
            PreviewSurface::Peek => self.file_peek_subject()?.path?,
            _ => self.preview_picture(surface)?.path.clone(),
        };
        animation::path_names_an_animation(&path).then_some(path)
    }

    /// **Ask the worker for the frames of every animation on the glass** —
    /// the first of them for a file this window has not looked inside yet, and
    /// the next ones for a file it is playing (§7.44 ⑤; user report
    /// 2026-09-10).
    ///
    /// **Opened once per file and never again**: the answer is filed under the
    /// file's key whether it decoded or not, so a `.gif` that is one still frame
    /// — or one whose frames are too large to stream — costs exactly one walk of
    /// one container for as long as this window is open, and a still after that.
    ///
    /// **Refilled as often as the ring has room**, which is the streaming half:
    /// a file is longer than the memory a hover is worth, so what is held is a
    /// second or so of frames and the rest is fetched as the play head eats it.
    /// The request is posted from here — the pass that already runs every frame
    /// there is an animation in the window — rather than from the animation
    /// tick, because this is the side that knows which files are on the glass:
    /// an animation nobody is looking at is one this window stops decoding.
    ///
    /// **And only for the ones actually drawn** (adversarial review 2026-09-11,
    /// B8). `drawn` is [`Self::drawn_animations`]'s list — the same list the
    /// renderer was handed and the same list the clock runs over — so the set
    /// that is refilled and the set that advances cannot drift apart. They did,
    /// and the drift was the user's report: the clock walked the whole cache and
    /// this walk asked only for surfaces, so a `.gif` in a background tab drained
    /// its ring with nothing refilling it.
    ///
    /// **A file opened starts at its first frame** — the other half of the same
    /// report. A surface that has begun showing a file it was not showing before
    /// lets go of whatever playback of it this window was holding, and the
    /// worker opens the file again; the answer is a new
    /// [`AnimationEntry::Ready`] with a new serial, standing on frame zero. A
    /// surface that was merely not on the glass has not *begun* anything, so its
    /// entry is untouched and it resumes. `animation_presence` is the record
    /// that tells those two apart; see it for why drawn-ness cannot.
    fn request_animations(&mut self, drawn: &[DrawnAnimation]) {
        if !self.app.math_worker_running {
            return;
        }
        let showing: Vec<(PreviewSurface, Option<PathBuf>)> = self
            .animated_surfaces()
            .into_iter()
            .map(|surface| (surface, self.animation_path_of(surface)))
            .collect();
        let named: Vec<(PreviewSurface, Option<String>)> = showing
            .iter()
            .map(|(surface, path)| {
                (
                    *surface,
                    path.as_deref().map(normalized_local_image_path_key),
                )
            })
            .collect();
        for key in animations_opened(&mut self.window.animation_presence, &named) {
            // **Let go of the playback, keep the refusal.** Whether a file can
            // be animated at all is a property of the file and does not change
            // by being opened again, so asking a second time would be a walk of
            // a container this window has already declined. A decode already in
            // flight is likewise the answer to this open: it will land standing
            // on frame zero.
            if matches!(
                self.window.animations.get(&key),
                Some(AnimationEntry::Ready { .. })
            ) {
                self.window.animations.remove(&key);
            }
        }
        for (_, path) in &showing {
            let Some(path) = path else {
                continue;
            };
            let key = normalized_local_image_path_key(path);
            if self.window.animations.contains_key(&key) {
                continue;
            }
            let leaf = self.focused_shell_address();
            if self
                .app
                .math_worker
                .tasks
                .send(MathWorkerRequest::PeekAnimation {
                    leaf,
                    path: path.clone(),
                })
                .is_ok()
            {
                self.window.animations.insert(key, AnimationEntry::Pending);
            }
        }
        // **The refills, over the drawn set and each animation once.** Two
        // surfaces showing one `loading.gif` are one animation at one phase
        // (§7.44 ⑤), so they are also one order to the worker.
        let mut asked: BTreeSet<&str> = BTreeSet::new();
        for animation in drawn {
            if asked.insert(animation.key.as_str()) {
                self.request_animation_fill(&animation.key, &animation.path);
            }
        }
    }

    /// **Send one playing animation's decoder to the worker for as many frames
    /// as its ring has room for** (user report 2026-09-10).
    ///
    /// Nothing happens for an animation that wants nothing, which is the great
    /// majority of calls: a ring holding its second of play is a ring that asks
    /// for another frame only when the play head has eaten one.
    ///
    /// The entry is taken out of the map and put back rather than reached into,
    /// for the reason `bt_term::BoundedCache` states in its own note — an entry
    /// is weighed when it goes in, and a ring that shrank under the map would
    /// leave the ceiling counting pixels that had been dropped. Since B9 that
    /// re-weighing also counts the fill this call puts in the air: the
    /// reservation is made by `take_cursor` and the insert below is where the
    /// map learns about it.
    ///
    /// **Never twice for one animation**, and that needs no flag: the cursor
    /// *is* the record that a fill is in flight, so an animation whose cursor is
    /// away answers `frames_wanted() == 0` and this returns (adversarial review
    /// 2026-09-11, B10 — the decoration worker is an unbounded FIFO with no
    /// supersession, so a lane that could double-post would queue frames of one
    /// file in front of every other question on it).
    fn request_animation_fill(&mut self, key: &str, path: &std::path::Path) {
        let wants = matches!(
            self.window.animations.get(key),
            Some(AnimationEntry::Ready { animation, .. }) if animation.frames_wanted() > 0
        );
        if !wants {
            return;
        }
        let Some(AnimationEntry::Ready {
            serial,
            mut animation,
        }) = self.window.animations.remove(key)
        else {
            return;
        };
        if let Some((cursor, want)) = animation.take_cursor() {
            let leaf = self.focused_shell_address();
            let sent = self
                .app
                .math_worker
                .tasks
                .send(MathWorkerRequest::AnimationFill {
                    leaf,
                    path: path.to_owned(),
                    serial,
                    cursor,
                    want,
                });
            // **A request that could not be posted brings the cursor home.** The
            // lane is gone, so no completion is coming, and an animation whose
            // decoder went with the request would stand on its last frame for as
            // long as this window is open.
            if let Err(std::sync::mpsc::SendError(MathWorkerRequest::AnimationFill {
                cursor,
                ..
            })) = sent
            {
                animation.park_cursor(cursor, Vec::new());
            }
        }
        self.window
            .animations
            .insert(key.to_owned(), AnimationEntry::Ready { serial, animation });
    }

    /// **Where one surface's video is drawn, and in which stack** — the geometry
    /// half of [`Self::video_layers`], separated because the bar needs the same
    /// answer and two derivations of one rectangle is how a control bar ends up
    /// somewhere its picture is not.
    fn video_shape_of(
        &self,
        surface: PreviewSurface,
        scale: f32,
        now: Instant,
    ) -> Option<VideoShape> {
        match surface {
            PreviewSurface::Seat(leaf) => {
                let placement = (leaf.tab == self.id)
                    .then(|| {
                        preview_image_placement(
                            &self.seats,
                            &self.seat_layout,
                            leaf.seat,
                            scale,
                            self.window
                                .pane_motion
                                .transform_of(leaf.seat, now, self.app.motion),
                        )
                    })
                    .flatten()?;
                Some(VideoShape {
                    box_: placement.body,
                    clip: placement.clip,
                    // The pane painted this box a pass ago in its own body
                    // colour, and a second authority for it would be a second
                    // colour the day one of them is changed.
                    ground: None,
                    radius_px: 0.0,
                    stage: bt_render::VideoStage::Seat,
                })
            }
            PreviewSurface::Float(id) => {
                let body = self.float_body_rect(id, scale)?;
                let box_ = viewport_of_rect(body)?;
                Some(VideoShape {
                    box_,
                    clip: box_,
                    ground: Some(bt_render::background_rgb()),
                    // Zero, and that is not an oversight: `content_body` has
                    // already lifted this rectangle clear of the window's
                    // rounded floor, so the picture's own corners are square and
                    // the round ones belong to the face around it.
                    radius_px: 0.0,
                    // **Not the hole's level** — see [`WindowRuntime::float_video_level`].
                    stage: bt_render::VideoStage::Overlay(self.float_video_level(id)?),
                })
            }
            PreviewSurface::Peek => {
                let body = self.window.file_peek.as_ref()?.body?;
                // **Two grounds, and the card already chose between them.** A
                // recording's card stands its frame on the taller `page_ground`
                // (280×160, `PeekBody::Frame`) and a picture's on
                // `picture_ground` (280×120, `PeekBody::Image`) — so an
                // animation drawn into the recording's box would be a `.gif`
                // forty logical pixels lower than its own still. Asked of the
                // path, which is the same thing `peek_body_kind` asks.
                let ground = if self.animation_path_of(PreviewSurface::Peek).is_some() {
                    file_peek::picture_ground(body, scale)
                } else {
                    file_peek::page_ground(body, scale)
                };
                let box_ = viewport_of_rect(ground)?;
                Some(VideoShape {
                    box_,
                    clip: box_,
                    ground: Some(bt_render::background_rgb()),
                    // The card *does* round its own picture window, and nothing
                    // is painted behind it to round for it.
                    radius_px: file_peek::PEEK_IMAGE_RADIUS_LOGICAL_PX * scale,
                    // **An index the stack has not yet published draws
                    // nothing**, which is `VideoStage::Overlay`'s own rule and
                    // is exactly right for the one frame it can happen on: the
                    // card's level is written by the chrome pass, so the very
                    // first frame of a card that came up playing has no level
                    // yet. `usize::MAX` names no layer, the picture waits one
                    // frame, and the bar is laid out against the right box
                    // meanwhile.
                    stage: bt_render::VideoStage::Overlay(
                        self.window.file_peek_level.unwrap_or(usize::MAX),
                    ),
                })
            }
        }
    }

    /// **Hand the renderer this frame's video layers.** `true` when the list
    /// changed, which is when a frame is owed.
    ///
    /// The order is the design (adversarial review 2026-09-11, B8): the drawn
    /// animations are worked out **once**, that one list becomes the layers, the
    /// record of what is on the glass, and the set the refills are asked for.
    /// Three walks with three conditions is what let the clock and the decoder
    /// disagree about which animations were alive.
    pub(crate) fn refresh_video_layers(&mut self) -> bool {
        let drawn = self.drawn_animations();
        self.present_animations(&drawn, Instant::now());
        let layers = self.video_layers(&drawn);
        let changed = self.window.renderer.set_video_layers(layers);
        self.request_animations(&drawn);
        changed
    }

    /// **Record what is on the glass, and start the clock of anything that has
    /// just arrived on it** (adversarial review 2026-09-11, B8).
    ///
    /// An animation that was not presented last frame and is presented now has
    /// its next frame due a delay from *this instant*. That is one sentence for
    /// two defects. `animation::open` stamps its clock on the **worker thread**,
    /// a hand-off before any reader can see the frames, so a completion that
    /// landed in a busy turn was already late and the first tick walked the ring
    /// to catch up — a `.gif` that opened several frames in. And an animation
    /// that is not drawn does not advance, so its due time is a moment in the
    /// past by the time a reader comes back to it; without a rebase the first
    /// tick after a tab switch would fast-forward through however long the tab
    /// was away.
    fn present_animations(&mut self, drawn: &[DrawnAnimation], now: Instant) {
        let presented: BTreeMap<String, u64> = drawn
            .iter()
            .map(|animation| (animation.key.clone(), animation.serial))
            .collect();
        present_drawn_animations(
            &mut self.window.animations,
            &self.window.animations_drawn,
            &presented,
            now,
        );
        self.window.animations_drawn = presented;
    }

    /// **Move every animation on the glass to the frame that is due**, and say
    /// whether any of them changed (§7.44 ⑤).
    ///
    /// Advanced on the same tick a video's frames are collected on, and for the
    /// same reason: this is the pass that runs on a clock. `false` for the great
    /// majority of ticks — a hundred-millisecond frame at sixty hertz is five
    /// ticks out of six that owe nothing.
    ///
    /// **Over what was drawn, and not over the map** (adversarial review
    /// 2026-09-11, B8). The map is every animated file this window has looked
    /// inside, including the ones in tabs nobody is on; the refills are asked
    /// only for what is drawn, so a clock over the map was a clock running where
    /// no decoder was following it. What that cost is the user's report: a `.gif`
    /// left behind in another tab ate the second of frames it had queued, stopped
    /// one frame short of its ring, and resumed mid-file — then stood still until
    /// a worker round trip came back. An animation nobody is drawing is a picture
    /// holding still, which costs nothing and is the right thing to be showing
    /// the moment it is looked at again.
    fn advance_animations(&mut self, now: Instant) -> bool {
        advance_drawn_animations(
            &mut self.window.animations,
            &self.window.animations_drawn,
            now,
        )
    }

    /// Whether anything on the glass is an animation this window is running —
    /// what keeps the deadline live while a spinner spins.
    ///
    /// Read off the record of what was actually presented rather than walked for
    /// a fourth time (adversarial review 2026-09-11, B8): a window whose only
    /// animation is in a tab nobody is on has nothing to wake up for, and it is
    /// the same list that says so to the clock and to the decoder.
    fn an_animation_is_running(&self) -> bool {
        !self.window.animations_drawn.is_empty()
    }

    /// Fit **one** picture to the body its host gives it this frame, ask the
    /// worker for the raster that box wants, and answer with the pixels the seat
    /// pass is to draw — or with `None`, which is every other case there is.
    ///
    /// **Everything about a picture except where its pixels land is the same on
    /// both hosts** — the same path-keyed decode, the same zoom, the same
    /// [`image_destination`], the same request ledger — so the fork is the one
    /// `match` at the bottom and not a second copy of this function. That is the
    /// whole of the 2026-08-17 report: this arithmetic used to run for a seat
    /// only, so a preview pane torn off into a window kept its head, its foot and
    /// its meta line and showed nothing at all, and the request it had left in
    /// flight was then answered to nobody.
    ///
    /// **It returns the picture rather than writing it** (§7.1.6k⁷). The seat
    /// channel is a list, and a function that wrote into it one surface at a
    /// time would be back to the election that list exists to end: whichever
    /// pane refit last would be the only one on the glass. The caller collects
    /// the whole walk and hands it over once, exactly as it does for the
    /// documents and the videos.
    fn refit_preview_picture(&mut self, surface: PreviewSurface) -> Option<PreviewImage> {
        // **A frame is not drawn over the thing it was a picture of** (user
        // ruling 2026-08-27; §7.23 ⑩). Once the play verb has put an engine on
        // this pane, the pane's body is a hole with a browser composed under it,
        // and the still frame this window decoded is a rectangle of pixels that
        // would be painted straight over the moving one. The first refusal in a
        // function whose whole shape is refusals, and it is the same refusal:
        // this surface's picture is not on screen this frame.
        //
        // **The `PreviewImageState` is kept**, which is the reason this is a
        // refusal here and not a clearing at the play verb: it is the pane's
        // account of which recording this is, it is what the stop verb puts back
        // on the glass without a second decode, and it costs one comparison a
        // frame to leave it alone.
        //
        // **And the same refusal for an animation** (§7.44 ⑤): a `.gif` whose
        // frames have arrived is drawn by the video layer, a whole pass before
        // this one, and the still this function would fit is the same file at
        // the same size in the same box. Two of them is one painted over the
        // other, and the one on top is the one that does not move.
        if self.surface_is_playing_a_video(surface) || self.animation_running_on(surface) {
            self.hide_preview_picture(surface);
            return None;
        }
        let scale = self.window.renderer.metrics().scale_factor as f32;
        // **U8 — a seat's box travels with its pane's tween**, and carries the
        // crop a FLIP needs; a float's is its window's body, which does not
        // animate, because a window is not a pane and does not fly to a slot.
        // This is one sample of the clock for one commit; the frames in between
        // are re-placed by `redraw`, which is the only thing that runs per frame.
        let placement = match surface {
            // The tween and the solve are both this tab's, so a picture on a
            // pane of another tab has no placement here — the same `None` a
            // seat with no solved rectangle gets, reached by name rather than
            // by the seat number happening not to be in the tree.
            PreviewSurface::Seat(leaf) => {
                let Some(placement) = (leaf.tab == self.id)
                    .then(|| {
                        preview_image_placement(
                            &self.seats,
                            &self.seat_layout,
                            leaf.seat,
                            scale,
                            self.window.pane_motion.transform_of(
                                leaf.seat,
                                Instant::now(),
                                self.app.motion,
                            ),
                        )
                    })
                    .flatten()
                else {
                    self.hide_preview_picture(surface);
                    return None;
                };
                Some(placement)
            }
            PreviewSurface::Float(_) | PreviewSurface::Peek => None,
        };
        let Some(body_rect) = placement
            .map(|placement| {
                let body = placement.body;
                [
                    body.x as f32,
                    body.y as f32,
                    (body.x + body.width) as f32,
                    (body.y + body.height) as f32,
                ]
            })
            .or_else(|| self.preview_surface_body_rect(surface, scale))
        else {
            self.hide_preview_picture(surface);
            return None;
        };
        let Some(path) = self
            .preview_picture(surface)
            .map(|picture| picture.path.clone())
        else {
            self.hide_preview_picture(surface);
            return None;
        };
        let cache_key = normalized_local_image_path_key(&path);
        // **The store, and then the answer this surface is already standing on**
        // (§7.1.3u ③). A miss here used to read as *never asked*, and with more
        // pictures on the glass than [`MAX_PEEK_CACHE_BYTES`] holds that is a
        // loop with no input in it: each arrival evicts a decode another host is
        // drawing, the refit that arrival triggers finds the miss, asks again,
        // and the answer evicts the next.
        let (standing_content, standing_native, standing_stated) = self
            .preview_picture(surface)
            .map_or((None, None, None), |picture| {
                (
                    picture
                        .raster
                        .as_ref()
                        .map(|raster| raster.content_key.clone()),
                    picture.native,
                    picture.stated_size,
                )
            });
        let pixels = surface_pixels(
            &mut self.window.peek_cache,
            standing_content.as_deref(),
            standing_native,
            standing_stated,
            &cache_key,
        );
        let (content_key, native_rgba, native_width, native_height, reduced_from) =
            match pixels.clone() {
                SurfacePixels::Decoded {
                    content,
                    rgba,
                    native,
                    stated,
                } => (content, Some(rgba), native[0], native[1], stated),
                // **What it was told, when the store no longer holds what it was told
                // it from.** The raster on the glass stays there and the arithmetic
                // below runs on the decode's own dimensions exactly as it did when
                // the decode was in hand; what is missing is only the pixels a
                // sharper pass would be made from, and that is the errand at the foot
                // of this function.
                SurfacePixels::Standing {
                    content,
                    native,
                    stated,
                    ..
                } => (content, None, native[0], native[1], stated),
                SurfacePixels::Failed(reason) => {
                    // **A video that would not decode is not a failure of this pane** (user ruling
                    // 2026-08-27; §7.23). A file called `.png` that no decoder can read is something
                    // the reader should be told about, because there is nothing else to say about it;
                    // a `.webm` in a codec this machine has not got is an ordinary file, and the pane
                    // still has its length, its size and its name to state. So the sentence is
                    // withheld and the two fact lines carry the surface — see
                    // [`Self::video_meta_sentence`], whose degraded form exists for exactly this
                    // frame.
                    if !preview::path_names_a_video(&path)
                        && let Some(picture) = self.preview_picture_mut(surface)
                    {
                        // **The decoder's own reason, not this pane's guess**
                        // (owner's ruling 2026-09-12). A surface that opens a file
                        // this window has already refused reads its answer out of
                        // the store, and the store now remembers *why* — so a
                        // picture too long or too many pixels says which, here as
                        // much as on the surface that was watching when the answer
                        // arrived.
                        picture
                            .failure
                            .get_or_insert_with(|| PictureRefusal::decoded(&reason));
                    }
                    self.hide_preview_picture(surface);
                    return None;
                }
                SurfacePixels::Nothing { asked } => {
                    self.hide_preview_picture(surface);
                    if !self.app.math_worker_running {
                        if let Some(picture) = self.preview_picture_mut(surface) {
                            picture.failure = Some(PictureRefusal::this_window_cannot(
                                i18n::Text::PreviewFailedImageWorker.text(),
                            ));
                        }
                        return None;
                    }
                    if asked {
                        return None;
                    }
                    if self.request_peek_pixels(&path) {
                        self.window
                            .peek_cache
                            .insert(cache_key, PeekCacheEntry::Pending);
                    } else if let Some(picture) = self.preview_picture_mut(surface) {
                        picture.failure = Some(PictureRefusal::this_window_cannot(
                            i18n::Text::PreviewFailedImageWorker.text(),
                        ));
                    }
                    return None;
                }
            };
        // **What the recording is, when the pixels are only a frame of it** — see
        // [`PreviewImageState::stated_size`]. Read before the borrow below because it is a
        // lookup on the window, and `None` for everything that is not a video.
        let stated = preview::path_names_a_video(&path)
            .then(|| self.video_facts_of(&path).native)
            .flatten();
        if let Some(picture) = self.preview_picture_mut(surface) {
            picture.native = Some((native_width, native_height));
            // **A reduced picture parts the same two sizes a recording does**
            // (owner's ruling 2026-09-12), so it is filed in the same field: a
            // 6000×4000 photograph decoded down to the pixel budget is a file
            // whose picture is 6000×4000 and pixels that are not.
            picture.stated_size = stated.or(reduced_from);
        }
        // `.pv-image svg { max-width: 86%; max-height: 70% }` (mock-up 606).
        //
        // **The 30% of height the picture gives up is not slack** — it is where
        // the meta line under it stands, and the mock-up's `.pv-image` column is
        // the picture and that sentence with a 10px gap between them. Fitting to
        // the whole body would leave the line nowhere to go but on top of the
        // picture. A body too small to afford the fractions still gets them:
        // they are fractions, so they cannot starve it the way a fixed inset
        // could.
        //
        // **All of that is the `Fit` mode's, and `Fit` is only one of two.** Once
        // a surface has been zoomed the rectangle comes from
        // [`image_destination`] instead and may be many times the body; what does
        // not change is that the picture is the body's centre plus a pan, which
        // is why the painter is handed a displacement and not a corner.
        let _ = PREVIEW_BODY_INSET_LOGICAL_PX;
        // A body or a decode with no extent has no rectangle to argue about, and
        // the arithmetic below would answer with a one-pixel smear rather than
        // with the refusal this has always been.
        if body_rect[2] <= body_rect[0]
            || body_rect[3] <= body_rect[1]
            || native_width == 0
            || native_height == 0
        {
            if let Some(picture) = self.preview_picture_mut(surface) {
                picture.failure = Some(PictureRefusal::this_window_cannot(
                    i18n::Text::PreviewFailedSeatTooSmall.text(),
                ));
            }
            self.hide_preview_picture(surface);
            return None;
        }
        let image_px = [native_width, native_height];
        // **Re-clamped on the way out, and written back.** A pane made narrower
        // under a zoomed picture leaves a pan that is now past the end of its own
        // road; storing the clamped value here — rather than clamping only at
        // paint time — is what keeps the *next* gesture (a drag, a notch about a
        // pointer) starting from the place the eye is actually looking at.
        // **A video's still is fitted by the rule the video is *played* by, and
        // not by the picture channel's** (user ruling 2026-08-28; §7.42). The
        // two rules differ in one word — `bt_render::preview_image_extent` never
        // enlarges and `bt_render::video_fit_extent` fills — and the difference
        // is the whole of the `next12` defect: a 160×120 recording drawn at
        // 160×120 in a full-height pane, a postage stamp in a field of ground.
        // Route A fixed it inside the shell page's stylesheet (`object-fit:
        // contain`, §7.23 ⑪) where this window could not see it; this is the
        // same rule where it can, so pressing play does not move the picture.
        //
        // The pan goes with it. A still that is not zoomable has nothing to pan
        // — §7.23 ⑤ ruled that a frame does not zoom while it is not playing —
        // and a centred picture with a remembered displacement would be a
        // recording hanging off one edge of its own pane.
        let fitted_as_a_video = preview::path_names_a_video(&path);
        let zoom = self.preview_image_zoom(surface);
        let clamped_pan = if fitted_as_a_video {
            [0.0, 0.0]
        } else {
            image_clamped_pan(body_rect, image_px, zoom)
        };
        if clamped_pan != zoom.pan {
            self.preview_pane_mut(surface).zoom.pan = clamped_pan;
        }
        let zoom = ImageZoom {
            pan: clamped_pan,
            ..zoom
        };
        let drawn = if fitted_as_a_video {
            video_still_destination(body_rect, image_px)
        } else {
            image_destination(body_rect, image_px, zoom)
        };
        let display_width = (drawn[2] - drawn[0]).round().max(1.0) as u32;
        let display_height = (drawn[3] - drawn[1]).round().max(1.0) as u32;
        // **The CPU never upsamples.** Above 100% the resample target stops at
        // the decode's own pixels and the magnification is carried by
        // `display_*_px`, which the sampler already stretches; asking the worker
        // for an 8× raster would be sixty megabytes of RGBA for a picture whose
        // extra pixels do not exist. `preview_image_extent`'s `.min(1.0)` is
        // exactly that cap, which is why the target is still asked of it.
        let (cap_width, cap_height) = image_raster_cap(image_px);
        let Some((raster_width, raster_height)) = preview_image_extent(
            display_width.min(cap_width),
            display_height.min(cap_height),
            native_width,
            native_height,
        ) else {
            if let Some(picture) = self.preview_picture_mut(surface) {
                picture.failure = Some(PictureRefusal::this_window_cannot(
                    i18n::Text::PreviewFailedSeatTooSmall.text(),
                ));
            }
            self.hide_preview_picture(surface);
            return None;
        };
        let target = (content_key.clone(), raster_width, raster_height);
        let exact_raster = self
            .preview_picture(surface)
            .and_then(|picture| picture.raster.as_ref())
            .is_some_and(|raster| raster.matches(&target));
        // **The pixels, on whichever channel this host draws.** During a drag,
        // keep the last texture on screen and let the sampler stretch it to the
        // new fitted extent. It may be briefly soft, but the preview never
        // vanishes; quiet-time delivery below replaces it with a one-to-one
        // display raster.
        let held = self
            .preview_picture(surface)
            .and_then(|picture| picture.raster.as_ref())
            .map(|raster| {
                (
                    raster.key.clone(),
                    Arc::clone(&raster.rgba),
                    raster.width_px,
                    raster.height_px,
                )
            });
        // **What this surface contributes to the seat pass**, and `None` at both
        // of the other two answers. A pane whose raster has not arrived yet
        // contributes nothing — it is not "the slot, emptied", it is one entry
        // missing from a list, so a neighbour that does have its pixels keeps
        // them. And a float contributes nothing here at all: its window is an
        // overlay layer a whole pass above the seat pass, so a picture handed
        // down this channel would be drawn *behind* the very window that
        // contains it — the same sentence [`Self::refresh_preview_body`] already
        // makes about a float's document. It paints from the rectangle filed
        // just below instead.
        let produced = match (placement, held) {
            (Some(placement), Some((key, rgba, width_px, height_px))) => Some(PreviewImage {
                owner: picture_channel_owner(surface),
                seat: placement.seat,
                clip: placement.clip,
                key,
                rgba,
                width_px,
                height_px,
                display_width_px: display_width,
                display_height_px: display_height,
                pan_px: [clamped_pan[0].round(), clamped_pan[1].round()],
            }),
            (Some(_), None) | (None, _) => None,
        };
        if let Some(picture) = self.preview_picture_mut(surface) {
            picture.drawn = Some(drawn);
        }
        // **And what this surface must do about its pixels** (§7.1.3u ③). The
        // first answer is the one that ends the loop: a surface holding the very
        // raster this frame wants asks nobody anything, whatever either store
        // happens to hold this instant.
        let errand = picture_errand(&pixels, exact_raster);
        if matches!(errand, PictureErrand::Nothing | PictureErrand::Wait) {
            return produced;
        }
        if self.preview_picture(surface).is_some_and(|picture| {
            picture.pending.as_ref() == Some(&target) || picture.scale_settle_deadline.is_some()
        }) || !self.app.math_worker_running
        {
            return produced;
        }
        let Some(rgba) = native_rgba else {
            // **The pixels this pass would be made from are not in this window**
            // — the store let the decode go while this surface went on drawing
            // the raster it was resampled into. One read, and the picture stays
            // on the glass while it travels; the store's own `Pending` is what
            // keeps it to one. This is [`answer_one_picture`]'s own arm, said for
            // a pane instead of for a page.
            if self.request_peek_pixels(&path) {
                self.window
                    .peek_cache
                    .insert(cache_key, PeekCacheEntry::Pending);
            }
            return produced;
        };
        // **Every exact-size question this surface puts to the resample lane.**
        // One line per Lanczos3 pass asked for, with the size asked and the size
        // the last answer came back at, so a gesture's appetite can be counted
        // from the outside instead of guessed at.
        if self.app.trace_perf {
            let held_at = self
                .preview_picture(surface)
                .and_then(|picture| picture.raster.as_ref())
                .map_or_else(
                    || "none".to_owned(),
                    |raster| format!("{}x{}", raster.width_px, raster.height_px),
                );
            trace_sink::stderr_line(format!(
                "BT_PERF_TRACE image_resample want={}x{} held={held_at} display={display_width}x{display_height}",
                raster_width, raster_height,
            ));
        }
        let task = peek_scale_task(&target, rgba, native_width, native_height);
        if self
            .app
            .math_worker
            .scale_tasks
            .send(ScaleWorkerRequest::Preview {
                leaf: self.focused_shell_address(),
                task,
            })
            .is_ok()
        {
            if let Some(picture) = self.preview_picture_mut(surface) {
                picture.pending = Some(target);
                picture.failure = None;
            }
        } else if let Some(picture) = self.preview_picture_mut(surface) {
            picture.failure = Some(PictureRefusal::this_window_cannot(
                i18n::Text::PreviewFailedImageWorker.text(),
            ));
        }
        produced
    }

    /// What the window knows about its own picture, for
    /// [`chrome_tick_reuses_picture`].
    pub(crate) fn picture_on_glass(&self) -> PictureOnGlass {
        PictureOnGlass {
            frame_pending: self.window.pending_frames.pending_frame().is_some(),
            has_presented_frame: self.window.last_presented_frame.is_some(),
            // A tab with no shell holds nothing: the hold is a projection's
            // promise about a grid that is mid-reprint, and there is no grid
            // (§7.1.6h). `false` is the reading that lets a folder tab's chrome
            // tick reuse the picture, which is the only kind of frame it has.
            presentation_hold: self
                .focused()
                .is_some_and(|leaf| leaf.projection.presentation_hold()),
            content_revision: self.window.terminal_content_revision,
            presented_revision: self.window.presented_picture_revision,
        }
    }

    pub(crate) fn disable_preview_worker(&mut self) -> bool {
        preview::disable_preview_worker_state(
            &mut self.app.preview_worker_running,
            &mut self.app.preview_worker_notice_pending,
        )
    }

    /// Take every file head the preview worker has finished.
    ///
    /// The twin of [`Self::apply_files_results`], down to the shape of the
    /// silence: a tab that closed, or a buffer the pool has since evicted, has
    /// nowhere to put the answer, and that is not a failure — it is the
    /// cancellation §7.1.3 asks for, arriving as a dropped result.
    ///
    /// **The batch is the application's and this takes only its own out of it**
    /// (user report, 2026-08-23). It used to drain the receiver, which is one
    /// receiver for the whole process: the first window in the opening order took
    /// every pending answer, matched the ones addressed to another window's
    /// `TabId(1)` against its own tab of that number, found no buffer to put them
    /// in and dropped them — for ever, because `claim_head_read` had already been
    /// spent and nothing asks twice. In every window but the first, a markdown
    /// file opened into the preview pane drew its head, its foot and the two
    /// hairlines between them and not one word of itself.
    pub(crate) fn apply_preview_results(
        &mut self,
        batch: &mut Vec<preview::PreviewResponse>,
        lane_gone: bool,
    ) -> Result<()> {
        let mut changed = lane_gone;
        // Whether a read landing raised the "this file changed on disk" strip
        // over a document somebody had typed in — see [`preview::ReadLanded`].
        let mut said_disk_news = false;
        for response in answers_for(batch, |response| self.owns(response.owner())) {
            let Some(index) = self
                .window
                .tabs
                .iter()
                .position(|tab| tab.id == response.tab)
            else {
                continue;
            };
            // **A card is a surface too** (user ruling 2026-08-21). A
            // head read landing in a background tab used to owe nobody a
            // frame, because nothing of that tab was on screen; in focus
            // mode its card is, and the projection that would turn the
            // face into the document's first lines only runs on a frame
            // somebody asks for. `seats` answering is gates 1 and 2 of
            // `focus_thumb` — the mode is on and this card is inside the
            // column's clip box — read from the pass that just ran
            // rather than re-derived here.
            let carded = self.window.focus_thumbs.seats(response.tab).is_some();
            match response.answer {
                preview::PreviewAnswer::Head { outcome, base } => {
                    // **Where this document lives, and there are two places.**
                    // The tab's one pool; or — for a hover, which is not an open
                    // file and never enters the pool (P145) — the glance's own
                    // off-pool slot. Matched by source in both, so a pointer that
                    // has moved on to another row is the cancellation §7.1.3 asks
                    // for, arriving as a dropped result.
                    //
                    // **Through `land_read` and not through `accept`** (ticket
                    // T-EDIT-DISK): a read is answered against the body it was
                    // issued for, and a reader who typed while it was in flight
                    // keeps what they typed. The base travelled out with the
                    // question and came back with the answer, so what decides is
                    // the buffer's own state and not this frame's guess at it.
                    let landed = if let Some(buffer) = self.window.tabs[index]
                        .preview_pool
                        .get_mut(&response.source)
                    {
                        let landed = buffer.land_read(outcome, base);
                        Some((landed, buffer.content.clone()))
                    } else if let Some(peek) = self
                        .window
                        .peek_buffer
                        .as_mut()
                        .filter(|peek| peek.source == response.source)
                    {
                        let landed = peek.land_read(outcome, base);
                        Some((landed, peek.content.clone()))
                    } else {
                        None
                    };
                    // Evicted, never opened, and no card standing on it: nobody
                    // to file it against.
                    let Some((landed, content)) = landed else {
                        continue;
                    };
                    match landed {
                        // **And out by the one door**, whichever slot it landed
                        // in.
                        preview::ReadLanded::Took => {
                            changed |=
                                self.settle_landed_head(index, &response.source, content, carded);
                        }
                        // Nothing about the body moved, so nothing derived from
                        // it is owed a pass — and the caret is emphatically not
                        // healed against a body that was never installed. What
                        // may be owed is the strip that has just appeared over
                        // the document to say the file and this body have
                        // parted, and it goes through the same door the
                        // watcher's own news puts it up by: a strip is a row of
                        // somebody's document, so the seats are re-solved and
                        // not merely repainted.
                        preview::ReadLanded::Kept { said } => said_disk_news |= said,
                    }
                }
                // A picture's byte count, for the meta line under it.
                // Filed against the image state rather than the pool,
                // because a picture's buffer is not what is on screen —
                // the decode lane is. Told to **every** surface showing
                // that file: the byte count is a fact about the file, and
                // a second copy of the picture owes the same sentence
                // even though only one of them holds the texture.
                preview::PreviewAnswer::Size(bytes) => {
                    // A size is only ever asked of a picture, and a
                    // picture is a file.
                    let Some(answered) = response.source.file_path() else {
                        continue;
                    };
                    let tab = &mut self.window.tabs[index];
                    let mut told = false;
                    for (_, pane) in tab.preview_panes.iter_mut() {
                        if let Some(image) =
                            pane.image.as_mut().filter(|image| image.path == answered)
                        {
                            image.bytes = bytes;
                            told = true;
                        }
                    }
                    changed |= told && index == self.window.active_tab;
                }
                // **The glance's own slot again**, on the head read's reasoning
                // one arm up: a hover is not an open file, so its answer lands
                // here or nowhere. Matched by path, because a pointer that has
                // moved to another row has already replaced the question and the
                // stale answer is the cancellation arriving as a dropped result.
                preview::PreviewAnswer::PageCount(pages) => {
                    let Some(answered) = response.source.file_path() else {
                        continue;
                    };
                    if let Some(slot) = self
                        .window
                        .peek_facts
                        .as_mut()
                        .filter(|slot| slot.path == answered)
                    {
                        slot.answer = pages;
                        changed |= index == self.window.active_tab;
                    }
                }
            }
        }
        if changed {
            // **The whole body, not just the chrome around it.** A head read
            // landing is the one event that turns "Loading …" into lines, and
            // the lines are not chrome — they are the renderer's own preview
            // surface. Asking `refresh_chrome` alone was the first thing this
            // slice got wrong on a real window: the head named the file and the
            // body stayed empty, because nothing rebuilt the text after the
            // frame that had nothing to build it from.
            self.refresh_preview_for_layout();
            self.refresh_chrome();
            // And presented unconditionally, for the same reason: the chrome
            // genuinely may not have changed — the head already said the name
            // one frame ago — while the body has changed completely.
            self.present_chrome_change()?;
        }
        // **The strip a refused read raised**, through the door a strip is put
        // up by — [`Self::refresh_preview_file`]'s own line, for the same reason
        // it is written there: the band takes a row of the document under it, so
        // the seats are re-solved when the set of panes wearing one moves.
        if said_disk_news {
            self.settle_preview_disk_notices()?;
        }
        Ok(())
    }

    /// **The card's picture**: what shape to reserve for it, and the pixels if
    /// they have arrived.
    ///
    /// It rides the machinery the terminal's own image hover already owns — the
    /// path-keyed native decode in [`TabState::peek_cache`] and the one resample
    /// lane behind it — because a picture the pointer paused over is the same
    /// question whichever surface it was paused on, and a second decoder would be
    /// a second answer to it. What is the card's own is the box: the fit is
    /// against [`file_peek::PEEK_IMAGE_W_LOGICAL_PX`]'s frame, not against a
    /// pane's 86%.
    ///
    /// Asking is the *frame's* job here rather than the hover's, and that is not
    /// laziness: the card is rebuilt whenever the chrome is, the cache answers in
    /// one lookup once it is warm, and the two `pending` guards below mean a
    /// question already in flight is never asked twice.
    pub(crate) fn file_peek_picture(&mut self, path: &Path, scale: f32) -> file_peek::PeekBody {
        let (width, height) = self
            .file_peek_fitted_pixels(
                path,
                scale,
                (
                    file_peek::PEEK_IMAGE_W_LOGICAL_PX,
                    file_peek::PEEK_IMAGE_H_LOGICAL_PX,
                ),
            )
            .unwrap_or((0.0, 0.0));
        file_peek::PeekBody::Image { width, height }
    }

    /// **What this window knows about the video at `path`** — three optional numbers, and the
    /// default when nothing has come back yet.
    ///
    /// One reader for both surfaces, keyed exactly as the pixels beside it are, so the card and
    /// the pane cannot end up printing two different sentences about one file. A file nothing has
    /// been asked about yet answers `VideoFacts::default()`, which is three `None`s — the same
    /// answer a file whose decode has not landed gives, and the same one a file that never decodes
    /// keeps for its length and its resolution.
    pub(crate) fn video_facts_of(&self, path: &Path) -> preview::VideoFacts {
        self.window
            .video_facts
            .get(&normalized_local_image_path_key(path))
            .copied()
            .unwrap_or_default()
    }

    /// Every live buffer in this tab's pool, as the switcher lists them.
    ///
    /// **The dropdown is the honest inventory of hidden state** (P130,
    /// `DESIGN.md` §7.1.3): the pool's own order, every buffer in it, and each
    /// one's dirty bit — a list that showed only the clean ones, or only the
    /// recent ones, would be the window deciding which unsaved edits you are
    /// allowed to know about.
    /// **The pane the switcher is hanging under, if it is on the tab in front**
    /// (§7.12 ⓑ).
    ///
    /// The menu is the window's — one pointer, one popup — and the head it hangs
    /// from is a tab's. So the two halves of its [`LeafId`] are spent in
    /// different places: the tab half here, once, as a question about whether
    /// this menu is on screen at all; the seat half by every reader below, each
    /// of which is already about the tab on the glass. Before the key carried a
    /// tab, switching tabs with the switcher open re-pointed it at whichever
    /// pane the arriving tab had numbered the same.
    pub(crate) fn preview_menu_seat(&self) -> Option<SeatId> {
        let leaf = self.window.preview_menu.leaf()?;
        (leaf.tab == self.id).then_some(leaf.seat)
    }

    pub(crate) fn preview_menu_items(&self, seat: SeatId) -> Vec<profiles::PreviewMenuItem> {
        switcher_rows(
            &self.preview_pool,
            self.preview_pane(self.preview_here(seat))
                .and_then(|pane| pane.buffer.as_ref()),
            &self
                .app
                .pins_store
                .rows_of(&[bt_persist::PinKind::File, bt_persist::PinKind::Url]),
        )
    }

    /// **Go to a page, having asked whether this window may** — the second of
    /// the pin's two validations (`plan.md` §3「钉不是授权」).
    ///
    /// The first is at the moment of pinning ([`Self::toggle_switcher_pin`]);
    /// this is the one at the moment of *using*, and both are the same door
    /// (`webnav::address_bar`) because a store that could grant permission is a
    /// store whose permission is "whatever that file says". A pin written by an
    /// older build, edited by hand, or made before the policy tightened is
    /// refused here and the seat does not move — which is also §7.7 ③'s
    /// "什么都没发生比一个确认框更诚实" for a target this window will not go to.
    ///
    /// The engine asks a *third* time, inside `NavigationStarting`, because
    /// redirects and page scripts start navigations no address field ever saw
    /// (`webnav` ①). This one is about the string; that one is about the load.
    fn navigate_preview_page(&mut self, target: &str) -> Result<()> {
        let Some((url, minted)) = page_destination(target) else {
            eprintln!("BT_WEB refused {target}");
            return Ok(());
        };
        self.open_web_page_with(&url, minted)
    }

    /// **What the switcher stands on and what it would list** — its anchor and
    /// its rows — or `None`, which means this menu draws nothing at all.
    ///
    /// **A menu whose stand is `None` is not up** (P137's rule, third instance,
    /// 2026-09-21). One `&self` answer, read by both halves that used to decide
    /// it apart: [`Self::preview_menu_layout`] draws exactly when this is `Some`
    /// and [`Self::popups_up`] counts the popup as up exactly when it is, so the
    /// window cannot come to believe the keyboard is a menu's while the glass
    /// shows none — see `popups_up`'s own note for what that costs a reader.
    ///
    /// **The pill when the head wears one, else the name itself.** The two are
    /// one control (「名字即按钮」, 2026-08-19 — "the name answers the pointer
    /// whether or not it is a switcher"), and the pill is the switcher's
    /// *ground*: a head too narrow to seat the switcher's own two boxes drops
    /// them (`seats::preview_head_geometry`) and the name is then the whole of
    /// what was pressed. Hanging the list from what is actually drawn is what
    /// keeps the buffer list reachable at every width — by pointer and, through
    /// the swallow below, by keyboard.
    pub(crate) fn preview_menu_stand(
        &self,
        seat: SeatId,
    ) -> Option<([f32; 4], Vec<profiles::PreviewMenuItem>)> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let rect = seats::full_pane_rect(&self.seat_layout, seat)?;
        let head = seats::pane_head_geometry(
            rect,
            bt_layout::SeatKind::Preview,
            self.seat_layout.seat_is_on_stage(seat),
            scale,
        );
        let furniture = seats::preview_head_geometry(&head, scale, self.preview_head_tools(seat));
        let items = self.preview_menu_items(seat);
        if items.is_empty() {
            return None;
        }
        Some((furniture.pill.unwrap_or(furniture.name), items))
    }

    /// The switcher's box this frame, or `None` when it is shut.
    ///
    /// **Laid out against the live head, every frame** — E59/E60's rule, and
    /// P136 records the same bug arriving a *third* time in the prototype: "a
    /// detached anchor measures (0,0), which is the 'opens in the top-left
    /// corner' bug". Re-deriving from the current layout is what makes that
    /// impossible rather than unlikely, and it makes the other half free: an
    /// anchor that has gone folds the menu instead of measuring a rectangle that
    /// is no longer anywhere.
    pub(crate) fn preview_menu_layout(&mut self) -> Option<profiles::PreviewMenuLayout> {
        let seat = self.preview_menu_seat()?;
        let (anchor, items) = self.preview_menu_stand(seat)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
        Some(profiles::preview_menu_layout(
            anchor,
            (width as f32, height as f32),
            scale,
            &items,
            &mut measure,
        ))
    }

    /// The name button: open the switcher here, or shut it if it is already here
    /// (P136 — the second press on the same pane collapses it).
    pub(crate) fn toggle_preview_menu(&mut self, seat: SeatId) -> Result<()> {
        // A popup opening closes whatever else was up, and it is the opener that
        // does it (E61): mutual exclusion cannot be left to a press falling
        // through, because every opener stops its own press from travelling.
        self.close_popups_except(Popup::Preview);
        let here = self.leaf_here(seat);
        self.window.preview_menu.toggle(here);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **The one close path** (P133 — "so the chevron never gets stranded
    /// flipped; the root menu's lesson, applied on day one this time").
    ///
    /// There is nothing to un-flip because there is nothing holding a flipped
    /// state: the chevron's angle is derived from `preview_menu_seat()` on the
    /// next frame, so shutting the menu *is* turning it back.
    pub(crate) fn close_preview_menu(&mut self) -> Result<bool> {
        if !self.window.preview_menu.close() {
            return Ok(false);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Show a different buffer from the pool in this pane (P131).
    ///
    /// **The pool is the only source**, so switching is a change of *view* and
    /// nothing else: the buffer keeps its edits, its dirty bit and — through
    /// `open_preview_file`'s own filing — its caret and scroll, which is why
    /// §7.1.3 promises the switch costs "零提示零打断".
    pub(crate) fn choose_preview_row(&mut self, seat: SeatId, index: usize) -> Result<()> {
        // **The row decides which buffer, and whether there is one.** A kept
        // file nobody has opened has no pool entry (user ruling 2026-08-19), and
        // that row's press is an *opening* rather than a change of view — which
        // is what makes the PINNED section worth having on the first frame after
        // a restart, when the pool is empty and the list is not.
        let Some(item) = self.preview_menu_items(seat).into_iter().nth(index) else {
            return Ok(());
        };
        // **A page row is a navigation, always, and it goes through the gate.**
        // `plan.md` §3's third determinism clause: 从切换器选择 = 一次正常顶层导航
        // (进入/截断当前导航栈,不绕导航策略). So a page row does not restore a
        // buffer and does not read the pool — it asks `webnav::address_bar` for
        // permission exactly as a typed address would, and **a pin is not a
        // permission**: a row that came out of `pins.json` is a string somebody
        // may have edited by hand, may have written under an older policy, or may
        // have pinned before this one tightened.
        if item.is_page() {
            self.close_preview_menu()?;
            let Some(keep) = item.keep else {
                return Ok(());
            };
            return self.navigate_preview_page(&keep.target);
        }
        let Some(pool) = item.pool else {
            self.close_preview_menu()?;
            let Some(keep) = item.keep else {
                return Ok(());
            };
            return self.open_preview(PathBuf::from(keep.target));
        };
        self.close_preview_menu()?;
        // **The pool's own identity and the pool's own name.** The row was
        // drawn from this buffer, so re-deriving either from a filesystem would
        // be a second opinion about a question the pool has already answered —
        // and one that a buffer with no file behind it could not be asked.
        let Some((source, name)) = self
            .preview_pool
            .buffers()
            .nth(pool)
            .map(|buffer| (buffer.source.clone(), buffer.name.clone()))
        else {
            return Ok(());
        };
        if self
            .preview_pane(self.preview_here(seat))
            .and_then(|pane| pane.buffer.as_ref())
            == Some(&source)
        {
            return Ok(());
        }
        let Some(surface) = self.preview_landing_surface() else {
            return Ok(());
        };
        self.open_preview_source_on(surface, source, name)
    }

    /// The hand-off arrow's box on one preview seat, or `None` when that seat is
    /// not showing a page (or has no room for the control).
    ///
    /// Re-derived from the frame the seat is standing in, which is the
    /// `&self`-hit-test discipline [`Self::pane_chevron_box`] states at length:
    /// the rectangle a tip hangs off has to be the rectangle the button was
    /// drawn in, by one derivation and not by two that agree today. It is the
    /// hit test's own derivation, down to the stored measurement.
    pub(crate) fn preview_browser_box(&self, seat: SeatId) -> Option<[f32; 4]> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let rect = seats::full_pane_rect(&self.seat_layout, seat)?;
        // **On the rail since 2026-08-24.** The derivation is the same one — the
        // paint's, down to the stored measurement — and only the row changed:
        // the arrow stands beside the address it hands over.
        let _ = rect;
        self.rail_geometry(self.preview_here(seat), scale)?.external
    }

    /// The box one of a page's four head verbs is drawn in (§7.7 ②).
    ///
    /// The same derivation the paint and the hit test read, for their reason: a
    /// tip anchored on a second computation is a tip that stands beside the
    /// button rather than on it.
    pub(crate) fn preview_web_tool_box(&self, seat: SeatId, verb: WebHeadVerb) -> Option<[f32; 4]> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let rect = seats::full_pane_rect(&self.seat_layout, seat)?;
        // **Three of the four moved down** (user ruling 2026-08-24), so the tip
        // asks whichever row the button is drawn in — one derivation each, and
        // the same one the paint and the hit test read.
        match verb {
            WebHeadVerb::Back | WebHeadVerb::Forward | WebHeadVerb::Reload => {
                let rail = self.rail_geometry(self.preview_here(seat), scale)?;
                match verb {
                    WebHeadVerb::Back => rail.back,
                    WebHeadVerb::Forward => rail.forward,
                    _ => rail.reload,
                }
            }
            WebHeadVerb::DevTools => {
                let head = seats::pane_head_geometry(
                    rect,
                    bt_layout::SeatKind::Preview,
                    self.seat_layout.seat_is_on_stage(seat),
                    scale,
                );
                seats::preview_head_geometry(&head, scale, self.preview_head_tools(seat)).devtools
            }
        }
    }

    /// The slot a float's recording is drawn into — see
    /// [`WindowRuntime::float_video_level`], which is why this is not
    /// [`Self::float_hole_level`].
    fn float_video_level(&self, id: float::FloatId) -> Option<usize> {
        self.window.float_video_level.get(&id).copied()
    }

    /// **Every preview float this window is drawing**, bottom to top.
    ///
    /// Read off `drawn` rather than `live` for [`Self::float_holding_the_page`]'s
    /// reason: a window on its way out is still on the glass, and a band taken
    /// off it a frame early would leave a hole in a window still drawing itself.
    pub(crate) fn preview_float_ids(&self) -> Vec<float::FloatId> {
        self.window
            .float
            .drawn()
            .filter(|win| win.preview().is_some())
            .map(|win| win.epoch)
            .collect()
    }

    /// The buffer tenant, drawn — P43-P67's window.
    ///
    /// **The document does not go through the chassis's channels.** A float's
    /// quads and labels are the tenant's contribution *inside* the body rect, and
    /// a scrolled document is not a list of quads — it is a clipped surface with
    /// its own paragraphs, its own foot and its own caret. So it rides on
    /// [`marks::OverlayLayer::body`], which is drawn inside this layer and above
    /// this window's own face; handing it to the seat lane instead would paint it
    /// a whole pass earlier and therefore *behind* the window containing it.
    pub(crate) fn preview_float_layer(
        &mut self,
        id: float::FloatId,
        now: Instant,
    ) -> Option<marks::OverlayLayer> {
        let (geometry, fade) = self.float_geometry_of(id)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let surface = PreviewSurface::Float(id);
        let mode = self.window.float.drawn().find(|win| win.epoch == id)?.mode;
        // The head's caption and the foot's path, off whatever this window is
        // showing — a buffer, or a picture that came down the decode lane.
        let md_source = self.preview_md_source(surface);
        let (title, path, dirty, flip_to_source) = match self.preview_buffer_on(surface) {
            Some(buffer) => (
                buffer.name.clone(),
                // The foot's left hand is "where is this": a path for a file,
                // and the repository and the place in it for a document
                // composed out of one — the docked foot's own answer, on the
                // torn-off window, because it is the same strip
                // ([`Self::dress_preview_foot`]).
                buffer
                    .source
                    .file_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .or_else(|| buffer.source.composed_lead())
                    .unwrap_or_default(),
                buffer.dirty,
                // This window's own face, not the file's — a pane behind it may
                // be reading the same file the other way round.
                !md_source,
            ),
            None => {
                let image = self
                    .preview_pane(surface)
                    .and_then(|pane| pane.image.as_ref());
                (
                    image.map(PreviewImageState::title).unwrap_or_default(),
                    image.map_or_else(String::new, |image| {
                        image.path.to_string_lossy().into_owned()
                    }),
                    false,
                    false,
                )
            }
        };
        // **The row above takes the path, and the whole strip goes with it**
        // (§7.7 ⑩ 欠账, and the ruling of 2026-08-25 evening) — the docked
        // pane's own judgement, arriving in full on the one strip a window had
        // that a pane no longer does.
        //
        // The slice before this one retired the *left half* and kept the band,
        // on the argument that the band was still a button and still this
        // window's only channel for a flashed confirmation. The ruling spends
        // both: `Reveal in Explorer` is a named row of the breadcrumb's own
        // `Document` menu, and the confirmation is a corner tag over the body
        // (below) that costs the window no rows. What the report photographed
        // was what those two reasons left behind — an empty band with one folder
        // glyph in the corner of it.
        //
        // Found on the machine the hour the rail landed: the window's rail read
        // `…/bravo.html` while its foot, six pixels below, still read
        // `…/alpha.html` — one surface saying two things about where it is,
        // which is the exact duplication ⑩ opened this debt to avoid.
        //
        // [`float::FloatGeometry::wears_a_foot`] and not `rail.is_some()`: the
        // collapse is the chassis's answer, and asking it here a second way is
        // how two readers of one rule come to disagree.
        let footless = !geometry.wears_a_foot();
        let path = if geometry.rail.is_some() {
            String::new()
        } else {
            path
        };
        let revealed = self.foot_reveal_is_fresh(RevealedFoot::Float(id), now);
        // **A torn-off buffer confirms its own save** (user ruling, 2026-08-15,
        // as a consequence). "Saved" was drawn only in a docked pane's foot, and
        // a float's went to the body strip that the ruling retired — so without
        // this the word would have had nowhere left to be printed at all.
        let saved = self.preview_save_notice(surface, now) == Some(preview::preview_saved_notice());
        let wanted = if revealed {
            Some(foot_revealed_label())
        } else if saved {
            Some(preview::preview_saved_notice())
        } else {
            None
        };
        let (flash, foot_dissolved) = self.foot_saying(FootSaying::FloatPreview(id), wanted, now);
        let notice = self
            .preview_standing_fact(surface, now)
            .unwrap_or_default()
            .to_owned();
        let head_font = float::FLOAT_HEAD_FONT_LOGICAL_PX * scale;
        let foot_font = float::FLOAT_FOOT_FONT_LOGICAL_PX * scale;
        // **The run the words are cut to is the run they are drawn in.** With
        // the strip retired there is no `foot_path` left to cut against — it
        // collapsed with the rest of it — and the two phrases go to two places:
        // the flash into a bubble inside the body, the standing fact onto the
        // rail's right hand, which is where a docked pane's went when its own
        // strip retired. So the cut is made to the *bubble's* room, which is the
        // narrower of the two and therefore the honest one to measure against.
        // `dress_preview_foot` makes the same substitution for the same reason
        // one host over.
        let run = if footless {
            let margin = (seats::PAGE_HOVER_TAG_MARGIN_LOGICAL_PX * scale).round();
            let pad = (seats::PAGE_HOVER_TAG_PAD_X_LOGICAL_PX * scale).round();
            [
                geometry.body[0] + margin + pad,
                geometry.body[1],
                (geometry.body[2] - margin - pad).max(geometry.body[0] + margin + pad),
                geometry.body[3],
            ]
        } else {
            geometry.foot_path
        };
        let (title, foot) = {
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
            (
                // **Not upper-cased** (P51). The files head shouts its root
                // because a root is a place; a filename is a name, and a name
                // shouted is a name you have to read twice.
                settings::ellipsized(
                    &title,
                    geometry.head_title[2] - geometry.head_title[0],
                    head_font,
                    &mut measure,
                ),
                seats::dress_foot(
                    seats::FootDress {
                        dissolved: foot_dissolved,
                        run,
                        lead: &path,
                        flash: flash.as_deref(),
                        notice: &notice,
                        // Left-truncated, exactly as the docked foot is (P35):
                        // the ellipsis goes at the head so the file name
                        // survives.
                        cut_left: true,
                        font_px: foot_font,
                        gap_px: seats::FILES_FOOT_NOTICE_GAP_LOGICAL_PX * scale,
                    },
                    &mut measure,
                ),
            )
        };
        let hover = self
            .window
            .float_hover
            .filter(|(hovered, _)| *hovered == id)
            .map(|(_, part)| part);
        // **The row under the head, dressed exactly as a pane's is** (§7.7 ⑩
        // 欠账, 2026-08-25) — one `dress_preview_rail`, one measurement, one
        // record in `preview_rail_measures`, so the tip, the menu and the press
        // all read what this frame drew.
        // **And nothing is copied onto it from the foot since 2026-09-12**
        // (owner's ruling). The standing phrase used to be lifted off the foot
        // this frame had already dressed and hung on this row's right hand;
        // `dress_preview_rail` decides the padlock itself now, out of the same
        // buffer, so there is still exactly one derivation and no window in
        // which a pane and the window torn off it could say two different things
        // about one file.
        let rail = self.dress_preview_rail(surface, scale);
        let palette = bt_render::chrome_palette();
        // **What fills the body, decided by what is in it** (user report,
        // 2026-08-20).
        //
        // This used to be two `if`s and no question: the document pipeline below
        // and the picture channel further down. A commit graph is neither — it
        // is marks pushed straight into the body rectangle ([`git_graph::push_graph`])
        // — so a graph torn off into a window arrived as a head, a foot and
        // nothing between them, and there was nothing on the surface to say so
        // because a graph's `PreviewDocument` is empty *on purpose*.
        //
        // An exhaustive `match` on [`preview::PreviewChrome`] and not a ladder,
        // which is the ticket's other half: the next content kind that needs its
        // own machine must be a compiler error here rather than a fourth blank
        // window somebody finds by undocking one.
        let body = match self.preview_chrome_on(surface) {
            // The pipeline's own, on [`marks::OverlayLayer::body`] below — plus
            // **the one line a composed document prints instead of a body**
            // (G-3): a diff with nothing in it, or a repository that would not
            // answer. It rides the tenant's own channel because a float has no
            // seat placement for the docked pane's message lane to find, and
            // without it a torn-off empty diff is a blank window.
            //
            // Quiet ink, centred, at the docked notice's size: it is the same
            // sentence in the same voice, and a window is not a different kind
            // of surface for it to be said on.
            preview::PreviewChrome::Document => {
                // **A file this window cannot show wears the same card as a pane**
                // (§7.39, user report 2026-08-28: an unpreviewable file — a
                // `folio.exe` — was a card in the side pane and a blank window
                // here, head and rail over nothing). Drawn by the pane's own
                // painter given this window's body, so the two cannot come to look
                // different, and its one button is hit and pressed exactly as the
                // pane's is (`FloatPart::CardButton`). A document with no refusal
                // keeps the quiet centred notice an empty diff or unreadable
                // repository prints.
                if let Some(words) =
                    self.float_refusal_words(surface, self.preview_open_button_label(now))
                {
                    self.push_float_refusal_card(id, geometry.body, scale, &palette, &words)
                } else {
                    float::FloatBody {
                        quads: Vec::new(),
                        labels: self
                            .preview_buffer_on(surface)
                            .and_then(preview::PreviewBuffer::body_notice)
                            .map(|words| bt_render::ChromeLabel {
                                mono: false,
                                text: words.to_owned(),
                                rect: geometry.body,
                                font_size_px: bt_render::SEAT_TITLE_FONT_LOGICAL_PX * scale,
                                color: palette.body_hint_text,
                                align_right: false,
                                align_center: true,
                                letter_spacing_em: 0.0,
                                weight: bt_render::ChromeLabelWeight::Regular,
                                tabular_numerals: false,
                                clip: Some(geometry.body),
                            })
                            .into_iter()
                            .collect(),
                        sprites: Vec::new(),
                    }
                }
            }
            // Pixels, on this layer's own image channel further down — see the
            // note there for why a float cannot use the seat's texture lane.
            //
            // **Unless there are no pixels** (owner's ruling 2026-09-12): a
            // picture this window refused to draw is a page with nothing on it,
            // and it wears the pane's card — the sentence and the button that
            // hands the file to the machine — rather than the blank window a
            // `.png` too large to decode used to be here.
            preview::PreviewChrome::Picture => {
                match self.float_refusal_words(surface, self.preview_open_button_label(now)) {
                    Some(words) => {
                        self.push_float_refusal_card(id, geometry.body, scale, &palette, &words)
                    }
                    None => float::FloatBody::default(),
                }
            }
            // The picture that is made of marks, in this window's body rectangle
            // — the same three channels the docked pane's graph is drawn into,
            // built by the same `build_git_graph`.
            preview::PreviewChrome::Graph => self.push_float_graph(id, geometry.body, scale),
            // **A page has no second host** (slice (3) debt, DESIGN.md 7.8): one
            // seat, one controller, one visual in one composition tree, so a page
            // torn into a float has nowhere for its pixels to be. Drawing nothing
            // is the honest answer until a slice gives a float an engine of its
            // own; what stops anybody meeting it is that a web buffer is not
            // offered to the tear-off verb (`preview_can_pop_out`).
            preview::PreviewChrome::Web => float::FloatBody::default(),
        };
        let mut layer = float::build(
            &geometry,
            &float::FloatChrome {
                dissolved: foot_dissolved,
                mode,
                // `#i-file` and a right-hand dock panel: this window holds a
                // buffer, and P54's side-honesty puts the filled half where the
                // pane will land.
                mark: marks::ChromeMark::File,
                title: &title,
                path: &foot.lead,
                notice: &foot.notice,
                notice_width: foot.notice_width,
                dock_label: float_dock_label(),
                dock_mark: marks::ChromeMark::DockRight,
                hover,
                revealed: foot.flashing,
                dirty,
                flip_to_source,
            },
            body,
            scale,
            &palette,
            fade,
        );
        // **The confirmation is the news pill's now** (owner's ruling
        // 2026-09-12; §7.1.3x ②). It was a bubble in this corner from the
        // 2026-08-25 ruling that retired the foot — `Saved` and `Revealed…` in
        // the shape a docked page's hover line stands in — and that argument was
        // the right one said of the wrong surface: news of every kind belongs in
        // one place on every preview, and it is the pill over the bottom edge
        // that `notice_layers` lays out for both hosts. Drawing one here as well
        // would be the same word twice, which is the very thing the bubble was
        // guarded against when it was the only place there was.
        // **And drawn, on the window's own layer.** The chassis reserved the
        // band ([`float::FloatGeometry::rail`]) and the tenant fills it, which is
        // the division of labour this whole module is built on — the body's own
        // sentence, one strip higher.
        if let (Some(band), Some(frame)) = (geometry.rail, rail.as_ref()) {
            let mut quads: Vec<bt_render::ChromeQuad> = Vec::new();
            seats::push_preview_rail(
                band,
                seats::PreviewRailContent {
                    measure: &frame.measure,
                    address: &frame.address,
                    segments: &frame.segments,
                    open: i18n::Text::PreviewRailOpen.text(),
                    meta: &frame.meta,
                    flip_to_source: frame.flip_to_source,
                    web: frame.web,
                    edit: frame.edit.as_ref().map(|edit| seats::PreviewNameEdit {
                        text: &edit.text,
                        caret_px: edit.caret_px,
                        selection: edit.selection,
                        caret_lit: edit.caret_lit,
                        refused: frame.refused,
                    }),
                },
                match hover {
                    Some(float::FloatPart::Rail(part)) => Some(part),
                    _ => None,
                },
                scale,
                &palette,
                (&mut quads, &mut layer.labels, &mut layer.sprites),
            );
            float::lay_rail_on(&mut layer, quads);
        }
        layer.body = self.build_preview_body(surface);
        // **The picture, on this window's own mark channel** (user report,
        // 2026-08-17: "undock an image preview and the picture disappears").
        //
        // A seat's picture is drawn through `bt_render`'s `set_preview_images`
        // channel, which paints in the seat pass — a whole pass *below* the
        // overlays, which is where this window's own face is drawn. So a picture
        // handed to that channel by a float would be behind the very window that
        // contains it, and the honest answer is the one this layer's document
        // already uses: ride the layer. The glance card draws its thumbnail on
        // exactly this channel ([`file_peek::build`]), so a window's picture is
        // the same picture in the same kind of layer and not a second mechanism.
        //
        // Over the window's face and under its meta line, because a layer closes
        // its three channels in that order. **Clipped to the body**, because a
        // picture zoomed past 100% is larger than the box it is looked at
        // through, and an uncropped one would paint over this window's head, its
        // foot and the desk beside it — the crop is the float's answer to the
        // scissor a preview seat gets from its own viewport.
        if let Some(picture) = self.preview_picture(surface)
            && let (Some(rect), Some(raster)) = (picture.drawn, picture.raster.as_ref())
        {
            layer.images.push(bt_render::ChromeIcon {
                key: raster.key.clone(),
                rect,
                rgba: Arc::clone(&raster.rgba),
                width_px: raster.width_px,
                height_px: raster.height_px,
                opacity: 1.0,
                clip: Some(geometry.body),
                above_text: false,
            });
        }
        // **And the last frame of the page this window is carrying, while a
        // modal stands over it** (§7.8 ⑩). On this layer for the picture's own
        // reason one paragraph above — the page is composed under wgpu and this
        // window is drawn in the overlay stack, so a frame handed to the pane
        // lane would be behind the very window that contains it — and under the
        // modal for the reason every keepsake is: `float` sits below `modal` in
        // the stack, so the scrim dims this the way it dims the window round it.
        if let Some(icon) = self.float_page_keepsake_icon(id) {
            layer.images.push(icon);
        }
        Some(layer)
    }

    /// `.pv-popout` — the preview pane is carried out into a window of its own
    /// (P29/P60).
    ///
    /// **It moves the presentation, not the buffer.** The buffer stays exactly
    /// where §7.1.3 put it, in this tab's pool, so nothing is lost, no dirty gate
    /// has anything to ask about, and the switcher still lists it. What travels
    /// is the *view* — the [`PreviewPane`] whole, scroll, caret, notice and all —
    /// because a pop-out that put you back at the top of the file would be a
    /// re-open wearing a move's name.
    ///
    /// **A pop-out never kills a tab** (P61), and it is the layout layer that
    /// says so rather than a guard here: [`seats::Seats::close_seat`] refuses to
    /// empty a tree, so a pane that is the last one standing simply does not
    /// leave, and the view is put back where it was. That refusal is unreachable
    /// today for a third reason as well — a tab always holds at least one
    /// terminal (I106), so a preview leaf is never the only pane — but the
    /// structural answer is the one that survives that stopping being true.
    pub(crate) fn pop_out_preview(&mut self, seat: SeatId) -> Result<()> {
        let surface = self.preview_here(seat);
        // **The page travels too, and it is named here** (§7.14a). Read *before*
        // the seat leaves the tree, because after that there is no leaf to build
        // the name out of — and a page nobody can name is a browser that is
        // placed nowhere and then closed for having lost its pane.
        let carried = {
            let leaf = self.leaf_here(seat);
            self.window.web.contains_key(&leaf).then_some(leaf)
        };
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let viewport = self.float_viewport();
        // The button's own box, read while the head it stands in is still on
        // screen: the window hangs off the control that summoned it, which is
        // every other float's rule.
        let anchor = seats::full_pane_rect(&self.seat_layout, seat).and_then(|rect| {
            let head = seats::pane_head_geometry(
                rect,
                bt_layout::SeatKind::Preview,
                self.seat_layout.seat_is_on_stage(seat),
                scale,
            );
            seats::preview_head_geometry(&head, scale, self.preview_head_tools(seat)).popout
        });
        // **The size it was, capped** — the move's own argument applied to the
        // geometry. A document has no natural height to open at the way a tree's
        // row count is one, so the honest answer is "as tall as the pane you took
        // it out of", and `float_opening_size` cuts that to min(64vh, 520).
        let body_height = self
            .preview_surface_body_rect(surface, scale)
            .map_or(0.0, |body| body[3] - body[1]);
        let size = float::float_opening_size(
            float::float_height_for_body(body_height, scale),
            viewport,
            scale,
            float::FloatSizing::preview(),
        );
        // The menu hanging off this head is pointing at a head that is leaving.
        if self.preview_menu_seat() == Some(seat) {
            self.close_preview_menu()?;
        }
        let pane = self.preview_panes.remove(surface).unwrap_or_default();
        let metrics = self.seat_metrics();
        // **The pane leaves either way** (P61). Closing is the ordinary route and
        // it is refused for exactly one tree — the one with nothing else in it
        // (G84) — where the honest reading of a *move* is not "then nothing
        // happens" but "then the tab keeps a shell": `stand_in_terminal` puts a
        // default profile where the preview stood, and the buffer floats free.
        // Leaving the no-op there is what made the mock-up's window duplicate
        // itself (3838-3843), docked and floating at once.
        if !self.seats.close_seat(&metrics, seat) {
            // **The shell is spawned before the tree is touched**, against the
            // slot the preview is standing in this very frame — which is the slot
            // the stand-in inherits unchanged, because `ReplaceSeat` swaps the
            // leaf inside the slot and moves no rectangle. Nothing here is
            // invented (L10): it is the rectangle already on screen, and
            // `settle_seat_set_change` below re-solves and tells the shell its
            // real columns the ordinary way. Doing it in this order is what keeps
            // the failure clean — a `create_leaf_session` that cannot start a
            // ConPTY leaves the pane exactly where it was.
            let Some(body) = seats::pane_body_viewport(&self.seats, &self.seat_layout, seat, scale)
            else {
                *self.preview_panes.entry(surface) = pane;
                return Ok(());
            };
            let wake = &self.window.pty_wake;
            let formulas = FormulaSwitches::from_settings(self.app.settings_store.loaded());
            let scrollback = scrollback_quota(self.app.settings_store.loaded().scrollback_lines);
            // The **default** profile, which is what a stand-in is: it is not
            // inherited from anything, because the pane it replaces was never
            // running a shell to inherit from.
            let session = create_leaf_session(
                &self.window.renderer,
                body,
                LeafId {
                    tab: self.window.tabs[self.window.active_tab].id,
                    seat,
                },
                wake,
                None,
                &LeafSeed::default(),
                &self.app.profile_programs,
                formulas,
                scrollback,
                self.app.settings_store.loaded().line_wrapping,
            )?;
            let Some(arrived) = self.seats.stand_in_terminal(&metrics, seat) else {
                *self.preview_panes.entry(surface) = pane;
                return Ok(());
            };
            self.sessions.insert(arrived, session);
            self.focused_leaf = arrived;
        }
        let placed = match anchor {
            Some(anchor) => float::float_placement(anchor, size, viewport, scale),
            None => {
                let left = ((viewport[0] + viewport[2]) / 2.0 - size[0] / 2.0).round();
                let top = ((viewport[1] + viewport[3]) / 2.0 - size[1] / 2.0).round();
                [left, top, left + size[0], top + size[1]]
            }
        };
        let placed = float::cascade_origin(placed, &self.taken_float_origins(), viewport, scale);
        let frame = float::clamp_pinned(placed, viewport, scale);
        let tab = self.id;
        // Origin `None` and no anchor kept: this window was **torn off**, not
        // summoned from a header, so there is no trigger to re-click it from and
        // nothing to re-place it against as content arrives.
        let id = self.window.float.open(
            float::FloatMode::Pinned,
            None,
            float::FloatTenant::Preview(float::FloatPreview { tab, page: carried }),
            frame,
            None,
            Instant::now(),
        );
        *self.preview_panes.entry(PreviewSurface::Float(id)) = pane;
        // **And the graph's own view, the same way and for the same reason.** A
        // `GraphView` is where the graph is scrolled to, which commit is folded
        // open, which row wears the selected ground and what is typed in the
        // search field — the document's equivalent of everything the sentence
        // above says the `PreviewPane` carries. Left behind, a pop-out would land
        // the reader at the top of a history they had walked a thousand commits
        // into, which is the "re-open wearing a move's name" this whole function
        // refuses. The repository's own cache is not moved and does not need to
        // be: it is keyed by root on the tab, and the window reads that tab.
        let active = self.window.active_tab;
        if let Some(view) = self.window.tabs[active].git_graph_view.remove(&surface) {
            self.window.tabs[active]
                .git_graph_view
                .insert(PreviewSurface::Float(id), view);
        }
        self.forget_dead_float_gestures();
        self.apply_pointer_cursor();
        self.settle_seat_set_change()?;
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// `DOCK`, on a window holding a buffer — the float becomes a preview pane
    /// again (P62).
    ///
    /// **It lands in the tab you are looking at**, which is the files flyout's
    /// own ruling (§7.1.2, 2026-07-17) and is the same sentence for the same
    /// reason: a window floats across tab switches precisely so it can serve you
    /// wherever you are, and sending it home would be the app deciding for you
    /// where you meant to put it.
    ///
    /// So the **buffer** may have to move house. The rule on a path collision is
    /// P122's and it is asked of P122's own function
    /// ([`preview::PreviewPool::merge_buffer`]): one buffer per file, dirty wins,
    /// a tie stays with the tab that was already holding it. It used to be spelt
    /// out here as its own two-tab case, which was the same sentence in a second
    /// place — and the two would have parted company the first time either was
    /// amended.
    pub(crate) fn dock_preview_float(&mut self, id: float::FloatId) -> Result<()> {
        let Some(origin) = self
            .window
            .float
            .live(id)
            .and_then(float::FloatWin::preview)
            .map(|preview| preview.tab)
        else {
            return Ok(());
        };
        let surface = PreviewSurface::Float(id);
        let Some(from) = self.window.tabs.iter().position(|tab| tab.id == origin) else {
            return Ok(());
        };
        // Where it lands, resolved **before** the window is taken apart: this may
        // have to mint a leaf, and a view lifted out of a window that then had
        // nowhere to put it down would be a view nothing is holding.
        let Some(landing) = self.preview_landing_surface() else {
            return Ok(());
        };
        // The view, out of whichever tab's plane was holding it — a float's pane
        // lives with the tab that opened the window, not with the one on screen.
        let Some(pane) = self.window.tabs[from].preview_panes.remove(surface) else {
            return Ok(());
        };
        let active = self.window.active_tab;
        if from != active
            && let Some(path) = pane.buffer.clone()
            && let Some(incoming) = self.window.tabs[from].preview_pool.take(&path)
        {
            self.window.tabs[active].preview_pool.merge_buffer(incoming);
        }
        *self.preview_pane_mut(landing) = pane;
        // **And the graph's own view, home with it** — `pop_out_preview`'s move
        // run backwards, so a window docked mid-history lands scrolled where it
        // stood with its open commit still open. It travels out of the tab that
        // was holding the window and into the tab the pane is landing in, which
        // is the same journey the `PreviewPane` above has just made.
        let landing_tab = self.preview_tab_index(landing);
        if let Some(view) = self.window.tabs[from].git_graph_view.remove(&surface) {
            // **And what it had already read, when the two tabs differ.** A
            // window floats across tabs by ruling (§7.1.2) and docks into the
            // one you are looking at, so this is the ordinary case and not an
            // edge: the `GraphState` is keyed by root on the tab, and a pane
            // landing beside a tab that has never heard of this repository would
            // blink back through "Reading the repository…" for a history it is
            // already holding. Cloned rather than moved, because another surface
            // in the old tab may still be reading the same graph.
            if landing_tab != from
                && let Some(state) = self.window.tabs[from].git_graphs.get(&view.root).cloned()
            {
                self.window.tabs[landing_tab]
                    .git_graphs
                    .entry(view.root.clone())
                    .or_insert(state);
            }
            self.window.tabs[landing_tab]
                .git_graph_view
                .insert(landing, view);
        }
        if let PreviewSurface::Seat(leaf) = landing {
            // `DOCK` is a button and this is the click on it: the pane you just
            // put down is the one you are looking at — which is why the leaf it
            // landed on is this tab's, and the focus goes to its seat.
            debug_assert_eq!(leaf.tab, self.id, "a dock lands in the tab on the glass");
            self.seats.set_focus(leaf.seat);
        }
        // **And the live page, home with the pane it came out on** (§7.14a).
        // The buffer travels as a value and the graph's view as a value; a page
        // is neither — it is a browser addressed by a leaf, and docking is a
        // change of address. `WebSeat::rehost` is that change: it is the one
        // door that writes the seat's own cached address in the same call that
        // moves the visual, so the *next* rebuild — a crash, an Evergreen
        // update — asks for a controller on the pane the reader can see.
        //
        // Nothing happens when the float carried no page, which is every
        // document: the pop-out only names one when there was one.
        self.dock_the_page_of(id, landing);
        // Wiped rather than dismissed — a preview float has no exit to play
        // (P49 ③), so an animation frame here would be the one place it did.
        self.window.float.wipe(id);
        self.forget_dead_float_gestures();
        self.sweep_preview_panes();
        self.apply_pointer_cursor();
        self.settle_seat_set_change()?;
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Put a file on the preview seat, down whichever of the two content lanes
    /// it belongs in.
    ///
    /// **The fork lives here and nowhere else.** Every entrance — a double
    /// click, Enter, the menu row, a click on an inline image — asks this one
    /// question, so the two lanes cannot come to disagree about what a `.png`
    /// is depending on how it was reached.
    ///
    /// **That sentence is about the two lanes below this door, not about which
    /// doors exist.** A `.html` reached from the files column arrives here and is
    /// shown as text, while a plain click on a printed `file:` link to one does
    /// not reach this function at all — it does nothing, and `Ctrl` sends the
    /// page to the browser (§7.1.5g, user ruling 2026-08-20). That is a ruling
    /// and not an oversight: a URI in the output is somebody else's *reference to
    /// a page*, and the page — not its source — is what the press was for; a row
    /// in this window's own tree is a file the user picked out of a tree they
    /// rooted, and looking inside it is the whole of what that door is for. When
    /// W2 gives the preview seat something that renders a page, the exception
    /// ends and both doors come back together.
    pub(crate) fn open_preview(&mut self, path: PathBuf) -> Result<()> {
        self.open_preview_at(path, None)
    }

    /// [`Self::open_preview`] told **where in the file** the reference pointed
    /// (§7.1.5j, `path:line[:col]`).
    ///
    /// The line is spent on the document lane and nowhere else: a picture has no
    /// lines, so `sunset.png:12` opens the picture and the `12` is a coordinate in
    /// a space that does not exist. Only the column is dropped rather than
    /// honoured — the seat has one caret and putting it at a column would claim a
    /// precision the reference's *printer* rarely has; landing on the line is what
    /// every editor's `+N` does with a `file:line` and it is what the reader asked
    /// for.
    pub(crate) fn open_preview_at(
        &mut self,
        path: PathBuf,
        at: Option<bt_transcript::paths::PrintedPathLocation>,
    ) -> Result<()> {
        match preview_open_lane(&path) {
            // A video's frame has no lines either, so `at` is spent on it the
            // way it is spent on a picture and on a page.
            PreviewOpenLane::Picture | PreviewOpenLane::Video => {
                return self.open_preview_image(path);
            }
            // A page has no lines of this window's either, so `at` is spent the
            // way it is spent on a picture: the reference opens the thing it
            // names and the coordinate belongs to a space that does not exist.
            PreviewOpenLane::Page => return self.open_preview_web_file(path),
            PreviewOpenLane::Document => {}
        }
        let Some(line) = at.map(|at| at.line) else {
            return self.open_preview_file(path);
        };
        // The surface is resolved once and used twice: the landing rule may mint a
        // leaf, and asking it a second time after the file has landed could name a
        // different pane than the one the file went to.
        let Some(surface) = self.preview_landing_surface() else {
            self.mouse_trace(|| "open_preview_at leave=no-landing-surface".to_owned());
            return Ok(());
        };
        self.open_preview_file_on(surface, path)?;
        self.aim_preview_at_line(surface, line)
    }

    /// Point one surface at one line of the document it is showing.
    ///
    /// **The source face, always.** A line number is a coordinate in the file's
    /// bytes, and a rendered markdown page has no such coordinate — its blocks
    /// carry no source lines and a paragraph is several lines joined. So a
    /// reference that names a line asks for the face that has lines; the flip is
    /// the view's own (ruling 8⑧, 2026-08-13), so this changes what *this* surface
    /// shows and nothing about the buffer or about any other surface on it.
    ///
    /// The scroll itself is deferred rather than performed, because the body is
    /// read off a worker: at this instant the pane knows the file's name and
    /// nothing about its lines. [`Self::settle_preview_goto`] spends the intent as
    /// soon as there is something to measure, which is this frame when the pool
    /// already held the file and a later one when it did not.
    fn aim_preview_at_line(&mut self, surface: PreviewSurface, line: u32) -> Result<()> {
        let pane = self.preview_pane_mut(surface);
        pane.md_source = true;
        pane.goto_line = Some(line);
        // Which spends the intent on the spot when the pool already held the file,
        // and leaves it standing when the bytes are still on the worker — see
        // [`Self::settle_preview_goto`], which every layout refresh walks.
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Spend a surface's pending "open at this line", if the bytes it needs have
    /// arrived.
    ///
    /// The line goes to the **top** of the body rather than being revealed by the
    /// minimum move [`Self::reveal_preview_caret`] makes: a pane that has just
    /// opened a file has not been anywhere in it, so there is no reading position
    /// to disturb, and the line the reference named is the first thing the reader
    /// wants to see with its own text under it.
    /// **Spend the press that could not have a caret yet** (T5 ①, §7.1.3t).
    ///
    /// [`Self::settle_preview_goto`]'s shape and its argument, one gesture over:
    /// eligible Markdown is complete on first display and seats a caret at once.
    /// A press during an outstanding load/recovery can still name an older body;
    /// its byte waits on the pane until the result arrives and is spent here
    /// — as a caret, with the keyboard, exactly as if the body had been there
    /// when the button went down.
    ///
    /// **The intent is dropped when the answer comes back "no".** A whole-file
    /// read can land and still leave the buffer read-only — past the 8 MB
    /// editing cap, or bytes that would not decode — and the buffer says so by
    /// no longer awaiting anything. The foot has the sentence for it (T2's
    /// channel); what must not happen is a press left pending for the life of
    /// the pane, ready to plant a caret the day something unrelated makes the
    /// file editable.
    fn settle_preview_caret(&mut self, surface: PreviewSurface) -> bool {
        let Some(offset) = self
            .preview_pane(surface)
            .and_then(|pane| pane.md_caret_wanted)
        else {
            return false;
        };
        if !self.preview_shows_live_markdown(surface) {
            if !self.preview_awaits_the_whole_file(surface) {
                self.preview_pane_mut(surface).md_caret_wanted = None;
            }
            return false;
        }
        // Through the same writes entering goes through, so that a caret placed
        // a frame late is a caret placed the same way as every other.
        self.seat_preview_caret(surface, offset, false)
    }

    fn settle_preview_goto(&mut self, surface: PreviewSurface) {
        let Some(line) = self.preview_pane(surface).and_then(|pane| pane.goto_line) else {
            return;
        };
        let Some(content) = self
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.content.clone())
        else {
            // The bytes are still on the worker. The intent stays on the pane.
            self.mouse_trace(|| format!("settle_preview_goto leave=no-body line={line}"));
            return;
        };
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let Some(body) = self.preview_surface_body_rect(surface, scale) else {
            // The surface has no rectangle yet, so it has no rows to count.
            self.mouse_trace(|| format!("settle_preview_goto leave=no-rect line={line}"));
            return;
        };
        // Counted from one, the way every printer of a `file:line` counts, and
        // clamped by `offset_at` to the last line a shorter file actually has.
        let index = line.saturating_sub(1) as usize;
        let offset = preview_edit::offset_at(&content, index, 0);
        // The *drawn* row, not the line number: a wrapped body puts line 40 well
        // below the fortieth row. A face with no rows of its own — a table, a diff
        // — answers `None`, and then the intent is spent on the caret alone rather
        // than left pending forever against a body that will never have lines.
        let row = self
            .preview_wrap(surface)
            .map(|wrap| wrap.row_of(index, 0).0);
        let scroll = self
            .preview_pane(surface)
            .map_or([0.0, 0.0], |pane| pane.scroll);
        let scrolled = match row {
            Some(row) => {
                let metrics = seats::preview_text_metrics(scale);
                let wanted = [
                    scroll[0],
                    metrics.padding_y + metrics.line_height * row as f32,
                ];
                self.clamped_preview_scroll(surface, body, scale, wanted)
            }
            None => scroll,
        };
        let pane = self.preview_pane_mut(surface);
        pane.caret = preview_edit::EditCaret {
            anchor: offset,
            caret: offset,
            desired_column: None,
            desired_x: None,
        };
        let moved = pane.scroll != scrolled;
        pane.scroll = scrolled;
        pane.goto_line = None;
        let rows = self
            .preview_wrap(surface)
            .map(preview_edit::WrapLayout::rows);
        self.mouse_trace(|| {
            format!(
                "settle_preview_goto surface={surface:?} line={line} row={row:?} rows={rows:?} \
                 scroll={scrolled:?} moved={}",
                u8::from(moved)
            )
        });
        // **The body is built from the offset, so a scroll written after it was
        // built is a number nothing has drawn.** Both callers rebuild before this
        // runs — they have to, because the row this lands on is only knowable once
        // the wrap exists — so the rebuild owed by the move belongs here, on the
        // one frame in a file's life that a goto is spent.
        if moved {
            self.refresh_preview_body();
        }
    }

    /// **Bring every picture in this window up to `now`** — the *service* half
    /// of a turn, which the frame gate has no business standing in front of
    /// (closure review O4, 2026-09-18).
    ///
    /// Every gated advancer in this window does up to three different things and
    /// only one of them is the pacer's: a **service** brings time-driven state up
    /// to `now`, a **sample** draws it at compose, and an **ask** requests a
    /// frame of this window's own. This is a service, and the whole of the O4
    /// finding is that it was standing on the wrong side of the line — these
    /// lines lived below `advance_strip_animation`'s display gate, so a
    /// neighbouring pane printing every five milliseconds refused them for as
    /// long as it kept printing and a `.gif` or a recording stood on whatever
    /// frame it happened to be holding. Measured on a real playback: frame 0
    /// after five seconds, against frame 50 when the same host is serviced on
    /// those same presents. The periodic was reporting it live the whole time;
    /// what it was not was moving.
    ///
    /// So it runs on **every** turn, above every gate, and again at the head of
    /// every compose ([`Self::carry_live_journeys`]) so that a frame composed for
    /// a keystroke or a drain shows the picture as of the instant it is of. That
    /// is safe because it is idempotent in `now`: a second call at the same
    /// instant pumps nothing, walks no clock and hands over no new layer.
    ///
    /// It costs nothing when nothing is live. A window with no recording sweeps
    /// an empty list and pumps an empty map; one drawing no animation has no
    /// clock to run ([`advance_drawn_animations`] walks what is *drawn*), and the
    /// layers are rebuilt only when something is there to rebuild them from.
    ///
    /// The debt it leaves is the one thing that has to outlive it. A frame that
    /// arrived is a frame the glass is owed, and the turn that pays it may be
    /// refused — so it is recorded on the window rather than returned, and
    /// [`Self::advance_strip_animation`] takes it when it is finally admitted.
    pub(crate) fn service_pictures(&mut self, now: Instant) {
        // **This turn's decoded pictures, collected** (route B slice ②; §7.44
        // ③).
        //
        // Here rather than in `redraw` because this is the pass that runs on the
        // clock: `redraw` runs when somebody asks for a frame, and a video that
        // waited to be asked would be a video that played only while the pointer
        // was moving. `pump` also settles each bar's armed hover intent, which
        // is the one wait in this window that has no other tick to ride on.
        //
        // The layers are recomputed whether or not a frame arrived, because the
        // *box* moves for reasons that are not the decoder — a pane in flight, a
        // float being dragged, a window resized — and a picture that stayed
        // where the layout used to be would be the FLIP's own defect with a
        // recording in it.
        let membership_moved = self.sweep_video_seats();
        let frames_arrived = self.window.video.pump(now) | self.advance_animations(now);
        // **And a box that is moving on a clock rather than on an event**
        // (closure review 2, 2026-09-18). Every *event* that moves a picture's
        // rectangle — a resize, a split, a tab switch, a scroll, a window
        // dragged — already ends in `refresh_preview_for_layout`, which asks
        // this question unconditionally from twenty doors. What that leaves is
        // the two rectangles that move without anybody touching anything: a pane
        // in FLIP and a float on its way in or out. Both are cheap to ask — a
        // walk of the panes that hold a tween, which is almost always none, and
        // of the floats that are drawn, which is almost always none.
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let boxes_are_moving = self.window.pane_motion.is_animating(now, self.app.motion)
            || self.window.float.is_animating(now, self.app.motion, scale);
        // **And what they were doing at the last decision**, because the frame a
        // travelling box most needs handed over is the one where it has stopped
        // — see [`pictures_need_handing_over`]. One `bool`, written on every
        // decision including the ones that hand nothing over, so a tween that
        // ended between two services owes exactly one final hand-over and an
        // ended one never comes back.
        let boxes_were_moving =
            std::mem::replace(&mut self.window.picture_boxes_were_moving, boxes_are_moving);
        // **Animations count here too**, and forgetting them was a real bug for
        // the length of one edit: a `.gif` with no recording anywhere in the
        // window would advance its frame, bump its generation, and never hand
        // the renderer the new layer — an animation that moved in the model and
        // stood still on the glass.
        // **And the tick that empties the list is a tick with something to say**
        // (§7.44 ⑨, photographed on the machine 2026-08-28).
        //
        // The first two clauses are the cheap gate they look like: a window with
        // no recording and no animation has no picture list to rebuild, and an
        // idle window should cost nothing. The third is the one that was
        // missing. `sweep_video_seats` runs one line above this and its whole job
        // is to *remove* seats — so the tick on which the last one goes is
        // precisely the tick where `self.window.video` is empty, the guard is
        // false, and the renderer is never handed the shorter list. It goes on
        // drawing what it was last given.
        //
        // Photographed: a floating window playing `clock.mp4` was closed, and
        // its last decoded frame stayed on the glass — no head, no bar, no
        // window around it, a rectangle of picture where a window used to be —
        // for three and a half seconds, until a hover card was dismissed and
        // `refresh_preview_for_layout` (which asks unconditionally) ran and swept
        // it away. The decoder had already stopped: four captures 700ms apart
        // were byte-identical over that rectangle.
        //
        // So the guard asks the renderer too. "Nothing is moving *and* the
        // renderer is holding nothing" is the real idle case, and it is still
        // one `is_empty()` on a slice.
        let anything_moving = !self.window.video.is_empty()
            || !self.window.animations.is_empty()
            || !self.window.renderer.video_layers().is_empty();
        // **And nothing at all when nothing has changed** (closure review 2,
        // 2026-09-18, P2). A service runs on every turn *and* at the head of
        // every compose, so a paused recording standing in a still pane was
        // rebuilding the whole layer list twice per five-millisecond present of
        // a neighbouring shell — the same pixels in the same rectangle, at four
        // heap allocations a time, with nothing in this window moving at all.
        // That is exactly the charge this work exists to remove: ordinary output
        // must pay nothing. The three reasons above are the whole of what can
        // make the list the renderer is holding wrong; if none of them holds,
        // what it is holding is right.
        if !pictures_need_handing_over(
            frames_arrived,
            membership_moved,
            boxes_are_moving,
            boxes_were_moving,
        ) {
            return;
        }
        let boxes_moved = anything_moving && self.refresh_video_layers();
        if frames_arrived || boxes_moved {
            self.window.pictures_owe_a_frame = true;
        }
    }

    /// Redraw the tab strip if anything in a mark slot has moved.
    ///
    /// Modelled on [`Self::advance_cursor_blink_if_due`] and for the same
    /// reason: the strip is rebuilt only when a channel has actually changed,
    /// so a window with nothing running costs nothing at all. What decides
    /// "changed" here is the animation itself — a breath and a spin move on
    /// every frame they are alive, and neither moves at all once its session
    /// stops working.
    pub(crate) fn advance_strip_animation(&mut self, now: Instant) -> Result<()> {
        // **[`pace::DEFAULT_FRAME_INTERVAL`] is a rate, and a rate nothing enforces is
        // not a rate.** `about_to_wait` runs after *every* event, not only when
        // a deadline expires — and the present each tick asks for is itself an
        // event. The arc moves far enough in half a frame to owe another, so
        // the ring free-ran at whatever the loop could turn: measured at **120
        // ticks a second against a declared 62.5**, with the swapchain's own
        // vsync as the only thing throttling it. Two frames of animation
        // composited onto a 60Hz design, at twice the price.
        //
        // The gate is the constant, read as what it says it is. A tick skipped
        // here is not a tick lost: [`Self::strip_animation_work`] is
        // clamped to the same clock, so the loop is woken exactly when the ring
        // is next allowed to move.
        // **The taskbar is mirrored above the rate gate, and it has to be.**
        //
        // [`pace::DEFAULT_FRAME_INTERVAL`] is a rate for *pictures*: it exists so an
        // arc does not ease twice per composited frame. The taskbar button is
        // not a picture this loop draws, it is a state this program is asserting
        // to the shell, and the frame it most needs to be right on is the one
        // where a run **ends** — which is exactly the frame nothing schedules,
        // because a window with nothing left animating reports no deadline at
        // all ([`Self::strip_animation_work`]). Under the gate, the last
        // reading before an idle window went quiet could be a green bar that
        // never comes down. Above it, the cost of a turn of the loop with
        // nothing happening is one fold over the sessions and one comparison
        // against [`TaskbarMirror::told`].
        let wanted = window_taskbar_progress(
            self.window
                .tabs
                .iter()
                .flat_map(|tab| tab.sessions.values())
                .filter_map(|leaf| leaf.session.status().progress),
        );
        // Borrowed apart rather than through `self`, because the mirror is a
        // field of the same window whose `window` handle it needs.
        let WindowRuntime {
            taskbar, window, ..
        } = &mut self.window;
        taskbar.show(window, wanted);
        if !strip_animation_tick_is_due(
            self.window.strip_animation_ticked_at,
            now,
            self.window.frame_clock.interval(),
        ) {
            return Ok(());
        }
        // **And the window's own display frame, above the strip's own rate**
        // (owner's report 2026-09-18). The gate above this one is the ring's
        // measured rate and this one is the glass's: a turn the loop was
        // not woken for — a shell speaking, a pointer moving — can be far enough
        // past the ring's sixteen milliseconds to pass it while the picture that
        // frame would replace has not reached the display yet. See
        // [`Self::animation_frame_is_due`], which is this gate generalised out of
        // this method to every animation this window runs.
        if !self.animation_frame_is_due() {
            return Ok(());
        }
        self.window.strip_animation_ticked_at = Some(now);
        // Every door of the attention queue, in one pass over one leaf at a time
        // — see `settle_attention` for why they stopped being two passes whose
        // order was the mechanism, and for the order the four that are left run in.
        // Sampled here beside the pass that reads it, and on the same turn: the facts that separate
        // "not in front of you" from "not on any screen" — and from "there is no taskbar to look
        // at" — are facts about *now*, and this pass is the one that decides what the reader is
        // owed.
        let place = sample_window_place(&self.window.window, self.window.window_focused);
        self.window.window_hidden = place.hidden;
        self.window.window_exposed = place.exposed;
        self.window.attention_sampled_at = Some(Instant::now());
        self.window.taskbar_auto_hidden = place.taskbar_is_auto_hidden;
        let mut raised: Vec<AttentionDelivery> = Vec::new();
        let switches = self.notification_switches();
        settle_attention(
            &mut self.window.tabs,
            self.window.active_tab,
            place,
            switches,
            &mut self.window.attention_next_place,
            now,
            attention_trace::global(),
            &mut raised,
        );
        self.raise_attention(raised)?;
        let active = self.window.active_tab;
        let motion = self.app.motion;
        let palette = bt_render::chrome_palette();
        let hovered = self.hovered_tab();
        let trigger_hovered = match self.window.seat_pointer.hover {
            Some(seats::ChromeTarget::TabFiles(index)) => Some(index),
            _ => None,
        };
        let peeking = self.peeking_tab();
        let mut owes_frame = false;
        for (index, tab) in self.window.tabs.iter_mut().enumerate() {
            // A new progress reading starts the arc easing toward it. This runs
            // for every tab, active or not: a background download's ring has to
            // keep reporting, and its tab is exactly the one the user cannot
            // otherwise see.
            tab.sync_ring(now);
            let (run, lit) = tab_trailing_targets(TabTriggerHand {
                pinned: tab.pinned,
                hovered: hovered == Some(index),
                on_trigger: trigger_hovered == Some(index),
                peeking: peeking == Some(index),
            });
            tab.pin_reveal.retarget(run, now, motion);
            // The reveal has to be *compared*, not merely sampled: `tab_owes_frame`
            // asks what would be drawn against what was drawn, and a width that
            // nothing compares would animate without ever scheduling a present.
            // Quantised to the 1/255 the sprite's own opacity resolves to, so a
            // tween settling in the last thousandth does not owe a frame forever.
            let drawn = tab.drawn_pin_reveal(now, motion);
            if tab.last_drawn_pin_reveal != Some(drawn) {
                tab.last_drawn_pin_reveal = Some(drawn);
                owes_frame = true;
            }
            tab.files_lit.retarget(lit, now, motion);
            let drawn = tab.drawn_files_lit(now, motion);
            if tab.last_drawn_files_lit != Some(drawn) {
                tab.last_drawn_files_lit = Some(drawn);
                owes_frame = true;
            }
            // The tab in hand is driven by the pointer and never by this clock,
            // so it is deliberately not passed here: a settle or a FLIP is the
            // only thing on this axis that moves on its own.
            let offset = tab.drawn_offset(now, motion, None).round() as i32;
            let landed = (tab.landing.sample(now, motion).0 * 255.0).round() as u8;
            if tab.last_drawn_offset != Some(offset) || tab.last_drawn_landing != Some(landed) {
                tab.last_drawn_offset = Some(offset);
                tab.last_drawn_landing = Some(landed);
                owes_frame = true;
            }
            let showing = tab.mark_state(index == active, now, motion, &palette);
            if tab_owes_frame(tab.last_drawn_mark, showing) {
                tab.last_drawn_mark = Some(showing);
                owes_frame = true;
            }
        }
        // The `˅` is the strip's own and belongs to no tab, so it settles its
        // debt outside the loop — but on exactly the same terms: the angle that
        // would be *drawn*, which is the quantized one, against the angle that
        // was. Comparing the raw fraction instead would owe a frame on every
        // wake-up of the 140ms, including the long tail where the mark does not
        // change at all.
        let turning = marks::ChromeMark::chevron(self.window.chevron_turn.sample(now, motion).0);
        if tab_owes_frame(self.window.last_drawn_chevron, turning) {
            self.window.last_drawn_chevron = Some(turning);
            owes_frame = true;
        }
        // R1/P168 — the rail's own debt, on the chevron's exact terms: what would
        // be *drawn*, not what the tweens hold. The width in whole physical pixels
        // and the fade in the 1/255 a sprite's alpha resolves to, because those are
        // the finest differences either can put on the glass, and a tween settling
        // through the long tail of its ease would otherwise owe a frame for the
        // whole of it.
        let drawn_rail = self.drawn_rail(now);
        if self.window.last_drawn_rail != Some(drawn_rail) {
            self.window.last_drawn_rail = Some(drawn_rail);
            owes_frame = true;
        }
        // The dock box's fade settles its debt on the same terms as the pin's:
        // the opacity that would be *drawn*, quantised to the 1/255 a layer's
        // alpha resolves to, against the one that was. It is read here rather
        // than inside the overlay build because a debt has to be noticed by the
        // thing that decides whether to build at all.
        let faded = self.drawn_dock_reveal(now, motion);
        if self.window.last_drawn_dock_reveal != faded {
            self.window.last_drawn_dock_reveal = faded;
            owes_frame = true;
        }
        // **B22 — the resizing cards' own debt**, on the same terms as the dock
        // box's: the inset that would be *drawn*, in whole physical pixels,
        // against the one that was. It is settled here rather than inside the
        // chrome build because a card running down after the button came up is
        // the one case where nothing else is asking for the frame — the pointer
        // has stopped, the layout has stopped, and only this transition is left.
        let carded = self.drawn_resizing_card(now);
        if self.window.last_drawn_resizing_card != carded {
            self.window.last_drawn_resizing_card = carded;
            owes_frame = true;
        }
        // **The popups' own debt** (the animation slice, 2026-08-26), on exactly
        // the resizing cards' terms and for a sharper version of their reason:
        // the last frame of a *departure* is the one nothing else in this window
        // asks for. The menu's state is already gone, the press that dismissed it
        // is long over and the pointer has stopped — what is left is a picture
        // fading in the arrival register, and only this reading knows it is
        // there. `drawn` is empty under reduced motion, so a window that asked
        // for stillness settles this debt once and never again.
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let passing = self.window.passages.drawn(now, motion, scale);
        if self.window.passages_drawn != passing {
            self.window.passages_drawn = passing;
            owes_frame = true;
        }
        // **The fades' own debt** (the animation slice's second half), on exactly
        // the same terms as the register above and with the same sharp case: a
        // pane the hand has *left* is a run fading out with nothing else in the
        // window moving — the pointer has already stopped, the layout has not
        // changed, and the only thing still happening is ninety milliseconds of
        // ink. Empty under reduced motion, so a window that asked for stillness
        // settles this once and never again.
        let settling = self.window.settling.drawn(now, motion);
        if self.window.settling_drawn != settling {
            self.window.settling_drawn = settling;
            owes_frame = true;
        }
        // And the feet's, on the same terms and with the same sharp case: a
        // receipt that has just run out is a strip dissolving with the pointer
        // long since still and nothing else in the window moving.
        let phrases = self.window.foot_phrases.drawn(now, motion);
        if self.window.foot_phrases_drawn != phrases {
            self.window.foot_phrases_drawn = phrases;
            owes_frame = true;
        }
        // **§7.1.6b′ F3 — the card column's entrance, settled on the same
        // terms.** Quantised to the whole physical pixel the slide can actually
        // move a rectangle by, for `drawn_rail`'s reason one paragraph up: a
        // tween settling through the long tail of an ease would otherwise owe a
        // frame for every one of the sixty in it while nothing on the glass
        // changed. Read here and not inside the chrome build because the last
        // frame of the entrance is the one nothing else asks for — the chord is
        // long over and the pointer never moved.
        let arriving = self.drawn_focus_reveal(now);
        if self.window.last_drawn_focus_reveal != Some(arriving) {
            self.window.last_drawn_focus_reveal = Some(arriving);
            owes_frame = true;
        }
        // The settings page's Advanced group, on the same terms: the frame its
        // height last drew at, against the one it would draw at now. Quantised
        // to a thousandth of the group's own height, which is finer than the
        // pixel it can move a row by — `settling`'s own argument about which
        // side of the glass to be wrong on.
        let disclosed = self.drawn_advanced_reveal(now);
        if self.window.advanced_reveal_sample != disclosed {
            self.window.advanced_reveal_sample = disclosed;
            owes_frame = true;
        }
        // **U8 — the panes' own debt, settled as it is asked.**
        //
        // Asked separately from "is anything moving?" for the reason
        // [`tab_owes_frame`] exists: the two part company on exactly the frame
        // the flight *ends*, where the FLIP has reached identity and reports
        // itself finished. A draw gated on movement would return before
        // rebuilding and leave every pane one eased step short of the box the
        // solver actually gave it — a permanent misalignment, because nothing
        // else is coming to correct it. `retire` afterwards, so a landed pane
        // stops carrying a rect from a layout two splits ago.
        // The live solve, and not a copy the tween took when it started: see
        // [`PaneMotion::settle_frame_debt`]. A window resized mid-flight moves
        // every pane, and the debt is about what is *drawn*.
        let pane_rects = self.pane_rects();
        let panes_owe = self
            .window
            .pane_motion
            .settle_frame_debt(&pane_rects, now, motion);
        self.window.pane_motion.retire(now, motion);
        // **The pictures' own debt, kept as a name of its own.** It has to
        // survive the chrome's question below, which is why it is not folded
        // into `owes_frame` and forgotten — see [`tick_owes_a_present`].
        //
        // **Taken rather than computed** (closure review O4, 2026-09-18): the
        // pictures are serviced above every gate now, so what reaches this line
        // is what [`Self::service_pictures`] found since the last tick that was
        // admitted — a frame that arrived during a refused turn is still owed
        // when one is finally allowed.
        let pictures_owe = std::mem::take(&mut self.window.pictures_owe_a_frame);
        // **§7.1.6b′ T-5 — and the card column's debt is a fourth, kept beside
        // them for the same reason.**
        //
        // This gate is where a card's refresh clock actually was. Everything
        // above it asks whether some *animation* moved, and a card is not an
        // animation: it is a picture of a pane, and the pane moves when the child
        // writes. So on a tick where nothing was easing, this line returned
        // before [`Self::refresh_chrome`] — which is the only door
        // `refresh_focus_thumbnails` is behind — and the column stood still.
        //
        // What made that survivable, and what made it look like a shell defect,
        // is the tab-mark breath: `tab_owes_frame(tab.last_drawn_mark, …)` a few
        // dozen lines up sets `owes_frame` on every frame of a mark that is
        // breathing, and only `OSC 133;C` starts one. So a PowerShell or Git Bash
        // card was carried through this gate by its own shell's marker for the
        // whole of every command, and a Command Prompt or WSL card was carried
        // through only by whatever else happened to be moving.
        //
        // The debt is **not** folded into the present's question below: whether
        // this tick owes the glass a picture is still `chrome_moved`'s to answer,
        // and a re-projection that drew the same rows draws nothing.
        let cards_owe = self.window.cards.owes_frame();
        let owes_frame = owes_frame || pictures_owe || cards_owe;
        if !owes_frame && !panes_owe {
            return Ok(());
        }
        // The strip decides for itself whether anything visibly moved. An
        // animation that is running but landed on the same pixels this frame —
        // a tween rounding to the same thousandth, a breath at the flat top of
        // its curve — still owes the *next* frame, which the deadline below
        // provides, but it does not owe a present now.
        //
        // A pane's debt is *not* answered by that question, and this is the one
        // place the two differ. The chrome is only half of what a pane FLIP
        // moves; the other half is the terminal's own viewport, which is built
        // in `redraw` from the same transform and which `refresh_chrome` knows
        // nothing about. A flight over a lone-headed pane can leave the chrome
        // byte-identical while the grid underneath it has to be drawn a hundred
        // pixels to the left.
        //
        // **And a decoded picture is a third debt, which this line used to
        // throw away** (the freeze of 2026-08-28; §7.44 ③). See
        // [`tick_owes_a_present`] for the whole of the argument.
        if !tick_owes_a_present(self.refresh_chrome(), panes_owe, pictures_owe) {
            return Ok(());
        }
        // Everything this pass can have moved is now in the renderer's hands or
        // in a pane transform the present re-samples. What is owed is a
        // present, and only when the picture underneath it is stale is it a
        // whole frame — see [`chrome_tick_reuses_picture`].
        self.publish_chrome_frame(now)
    }

    /// When the tab strip next needs waking, or `None` when nothing is moving.
    ///
    /// `None` is the important half: it is what lets `about_to_wait` fall back
    /// to `ControlFlow::Wait` and the process go genuinely idle. A strip with
    /// no working session, no indeterminate ring and no tween in flight asks
    /// for no wake-ups at all, which is why this is a deadline rather than a
    /// standing 60fps loop.
    /// The earliest instant [`Self::advance_strip_animation`] will do anything,
    /// given when it last did.
    ///
    /// Every animation deadline this window reports is clamped to it. Reporting
    /// an earlier one would wake the loop to be turned away by the rate gate
    /// and then re-arm the same deadline — a spin dressed as a schedule.
    fn strip_animation_next_tick(&self, interval: Duration) -> Option<Instant> {
        let strip = self
            .window
            .strip_animation_ticked_at
            .map(|last| last + interval);
        match (strip, self.next_animation_deadline()) {
            (Some(strip), Some(frame)) => Some(strip.max(frame)),
            (Some(strip), None) => Some(strip),
            (None, Some(frame)) => Some(frame),
            // Before either epoch exists the current turn is admitted and is
            // responsible for asking for the first picture. There is no timer
            // state to retain yet, so asking again must still answer `None`.
            (None, None) => None,
        }
    }

    /// The absolute frame appointment owned by the last successful present.
    /// This is the one answer every animated deadline in the fold is clamped to:
    /// never `now + frame`, always the epoch of a picture that reached the glass.
    pub(crate) fn next_animation_deadline(&self) -> Option<Instant> {
        self.window
            .frame_clock
            .deadline(self.window.last_present_at)
    }

    /// The gate's live answer, retained separately from the deadline getter:
    /// before the first present the current turn is due immediately, while the
    /// fold has no absolute frame epoch to retain yet.
    #[allow(dead_code)]
    pub(crate) fn next_animation_frame(&self, now: Instant) -> Instant {
        self.window
            .frame_clock
            .next_frame(self.window.last_present_at, now)
    }

    /// A state clock spent behind the frame gate cannot run before the glass is
    /// ready, but the clamp itself must remain an absolute instant.
    pub(crate) fn clamp_animation_deadline(&self, deadline: Instant) -> Instant {
        self.next_animation_deadline()
            .map_or(deadline, |frame| deadline.max(frame))
    }

    /// **Whether an animation may draw its frame on this turn**, and the gate
    /// that books the turn it may draw on when the answer is no.
    ///
    /// The other half of [`Self::next_animation_deadline`], and neither half works
    /// alone. The deadline says when the loop should be woken; this says what a
    /// turn that was *not* woken by it may do — and the loop is turned by
    /// everything, so without this an animation still advances at whatever rate
    /// the machine can compose at, and its own deadline is never the thing that
    /// wakes it. It is the gate `advance_strip_animation` has kept for the ring
    /// since the ring was measured at 120 ticks a second against a declared
    /// 62.5, asked here on behalf of every animation instead of one.
    ///
    /// **One answer for the whole turn**, decided at its head by
    /// [`pace::FrameClock::open`] and merely read here — this is not the live
    /// question, and it must not be. A turn runs several animations in a row and
    /// each of them may present; asked live, the first one to reach the glass
    /// would refuse every one after it, so a formula's height would move on this
    /// frame and the two marks riding its rectangle on the next. That is a worse
    /// stutter than the one this repairs.
    ///
    /// **And it decides who has to ASK for a frame, never who is carried by
    /// one** (review 2026-09-18, P1). The clock behind it is every present this
    /// window makes, a shell's output included, so a pane printing every five
    /// milliseconds refuses this for as long as it keeps printing — which is
    /// right, because a picture reached the glass a moment ago and the animation
    /// does not need one of its own. It is right only because
    /// [`Self::carry_live_journeys`] makes that picture carry the animation. See
    /// [`crate::pace`].
    ///
    /// A refusal is recorded rather than dropped: see [`pace::FrameClock::refuse`].
    /// **Asking costs nothing and marks nothing** (review 2026-09-18 round 2).
    /// It used to record that an animation was running, on the reasoning that
    /// only a running one would ask; four advancers ask before they have
    /// established that they hold anything at all, so a window with an empty tip
    /// host marked itself running for ever. What is mid-flight is reported once
    /// a turn from the journeys' own predicates — see [`Self::running_journeys`].
    ///
    /// # What may stand behind this, and what may never (closure review O4)
    ///
    /// Every advancer in this window does up to three different things, and only
    /// the third of them is this gate's business. Putting one of the first two
    /// behind it is not a saving, it is a stall:
    ///
    /// 1. **Service** — bring time-driven state up to `now`: pump a decoder,
    ///    walk a decoded animation's clock to the frame that is due, integrate
    ///    the distance an auto-scroll has travelled, retire what has ended.
    ///    Cheap, idempotent in `now`, and **never gated**: it runs on every turn
    ///    and again at the head of every compose
    ///    ([`Self::carry_live_journeys`]), so whatever frame goes out, for
    ///    whatever reason, is a picture of now. The O4 finding is two services
    ///    that were standing here — [`Self::service_pictures`] and
    ///    [`Self::service_drag_autoscroll`] — where a neighbouring pane printing
    ///    every five milliseconds held a playing recording on one frame and a
    ///    hand at the edge of a list still.
    /// 2. **Sample and draw** — a pure function of state and `now`, done at
    ///    compose and never here at all.
    /// 3. **Ask for a frame of this window's own** — the only thing this gate
    ///    decides, and for a periodic the ask is "one per display interval while
    ///    my condition holds", which a flood's own frames already satisfy.
    ///
    /// A fade has no service: it is a number read off a clock, so there is
    /// nothing to bring forward and nothing to lose by a refusal. That is why
    /// eleven of the thirteen advancers are gated whole.
    pub(crate) fn animation_frame_is_due(&mut self) -> bool {
        if self.window.frame_clock.admits() {
            return true;
        }
        self.window.frame_clock.refuse();
        false
    }

    pub(crate) fn strip_animation_work(&self, now: Instant) -> AnimationWork {
        let motion = self.app.motion;
        let tabs_moving = self.window.tabs.iter().any(|tab| {
            tab.mark_is_animating(now, motion)
                || tab.pin_is_animating(now, motion)
                || tab.flip.sample(now, motion).1
                || tab.landing.sample(now, motion).1
        });
        // The `˅` belongs to the strip and not to any tab, so a window with the
        // picker mid-turn and nothing else happening still has to be woken —
        // and, once the arrow lands, must stop being woken. Under reduced
        // motion this is never true: the turn has no frames to ask for.
        let chevron_turning = self.window.chevron_turn.sample(now, motion).1;
        // The dock box's fade is the one animation in this window that can be
        // running while the pointer is still: a drag that comes to rest over open
        // air still has 100ms of fade to finish, and nothing else would wake the
        // loop to draw it.
        let dock_fading = self
            .window
            .drop_preview
            .as_ref()
            .is_some_and(|shown| shown.reveal.sample(now, motion).1);
        // B22, and the same argument one line further: the cards keep running
        // down for a hundred milliseconds after the divider is let go, with the
        // pointer already still. Nothing else would wake the loop to draw them.
        let cards_moving = self.window.resizing_card_transition.sample(now, motion).1;
        // §7.1.6b′ F3, and the same argument once more: the card column's
        // entrance runs for 180ms after a chord that moved no pointer, and
        // nothing else in the window is moving while it does. `None` under
        // reduced motion, where `RevealTween` reports the target and asks for
        // nothing — which is what keeps a mode toggle as instant as it was
        // before it could animate.
        let focus_arriving = self.window.focus_reveal.sample(now, motion).1;
        // P168, and the same argument again: a rail that has been left behind by
        // the pointer keeps sliding shut with nothing else in the window moving,
        // and its labels keep fading for 100ms after that. Under reduced motion
        // neither is ever true — `RevealTween` reports the target with no frames
        // asked for — which is what makes `Reduced` a genuinely idle window and
        // not a fast animation.
        //
        // The fold sits **outside** that `draws_icon_rail()` gate, and has to:
        // a rail folding away is one whose `collapsed` is already true, which is
        // exactly the case that predicate answers `false` for. Inside the gate,
        // the fold would be a transition nobody ever woke the loop to draw — the
        // panel would sit still until some other event happened to ask for a
        // frame, which is the snap this animation exists to remove.
        let rail_moving = (self.window.rail.draws_icon_rail()
            && (self.window.rail_open.sample(now, motion).1
                || self.window.rail_text.sample(now, motion).1))
            || self.window.rail_fold.sample(now, motion).1;
        // **And the labels' sixty milliseconds of waiting, which is a wake and
        // not a motion** (closure review, 2026-09-18). Q183 hangs a
        // `transition-delay` on the way open — the words hold still until the
        // panel is wide enough to carry them — and the loop has to be woken for
        // the instant the fade begins. It is the one reveal in this window with
        // a delay, so it is the one place the two questions part company: see
        // [`RevealTween::owes_a_wake`].
        let rail_waiting =
            self.window.rail.draws_icon_rail() && self.window.rail_text.owes_a_wake(now, motion);
        let rail_wait_deadline = rail_waiting
            .then_some(self.window.rail_text.started)
            .flatten();
        // U8 — the active tab's panes, on the same terms and with the same
        // `None`: a window whose split has settled asks for no wake-ups at all,
        // and under reduced motion there was never a tween to ask for one.
        // Folded in here rather than added as a second deadline at the call site
        // because it answers the same question — "when does this window next
        // need waking for an animation?" — and two answers to one question is
        // how one of them gets forgotten.
        // C33, and the same argument once more: a disclosure triangle is still
        // turning for 120ms after the click that started it, with the pointer
        // already still and nothing else in the window moving. Under reduced
        // motion `RevealTween` reports the target with no frames asked for, so
        // this is never true and the window stays genuinely idle.
        //
        // **Every tree that can turn one, and that is the fix** (user report
        // 2026-08-28, `docs/DESIGN.md` §7.37 ④). This asked the docked columns
        // alone, and a *float's* tree — the folder flyout, the torn-off column —
        // keeps its cache on the window rather than in `file_trees`. So a
        // triangle clicked in one started its turn, drew its first frame, and
        // then asked nobody for a second: the loop went idle mid-animation and
        // the row sat with a shut triangle over its own open children until some
        // unrelated event happened to repaint. Measured before this line: click
        // a folder in a flyout and its `▸` stays `▸`; move the pointer anywhere
        // over the tree and it is `▾` on the next frame. That is the symptom
        // `a_row_a_locate_opened_arrives_turned_rather_than_turning` was written
        // about one surface earlier, arriving on the one surface whose cache
        // this question could not see.
        let files_turning = self.window.tabs[self.window.active_tab]
            .file_trees
            .values()
            .any(|cache| cache.any_turning(now, motion))
            || self
                .window
                .float
                .drawn()
                .filter_map(float::FloatWin::files)
                .any(|files| files.cache.any_turning(now, motion));
        // §7.7 ②, and the same argument once more: a page's mark spins while a
        // navigation is in flight, and nothing else in the window is moving
        // while it does — the engine reports when the navigation *ends*, not
        // sixty times a second on the way. Under reduced motion the phase is
        // pinned at twelve o'clock, so the arc is a still picture and asks for
        // no frames at all, which is what keeps `Reduced` a genuinely idle
        // window rather than a fast animation.
        let page_loading =
            motion != Motion::Reduced && self.window.web.values().any(|web| web.page().loading);
        // The animation slice's own, and the same argument once more: a menu's
        // entrance runs for 140ms after the press that raised it and its
        // departure for 90ms after the one that dismissed it, with the pointer
        // already still and nothing else in the window moving. Under reduced
        // motion the register holds nothing at all, so this is never true and a
        // window that asked for stillness is genuinely idle rather than quickly
        // animated.
        let passing = self.window.passages.moving(now, motion);
        // The second half of the same argument, for the fades: a head run going
        // out under a pointer that has already stopped, a Git row coming back to
        // full after its write landed, a tab's chrome finishing the handover, the
        // Advanced group still opening after the press that opened it. None of
        // those has anything else in the window asking for the frame, and none of
        // them is ever true under reduced motion.
        let fading = self.window.settling.moving(now, motion);
        // The feet's dissolves, and the same argument once more: a receipt going
        // back to being a path is ninety milliseconds with nothing else in the
        // window asking for a frame. Never true under reduced motion, where the
        // word swaps between two frames the way it always did.
        let saying = self.window.foot_phrases.moving(now, motion);
        // The settings page's Advanced group, and the same argument once more:
        // it runs for two hundred milliseconds after a press that moved no
        // pointer, and nothing else in the dialog is moving while it does. `None`
        // under reduced motion, where `RevealTween` reports the target and asks
        // for nothing at all.
        let disclosing = self
            .window
            .advanced_reveal
            .is_some_and(|(_, tween)| tween.sample(now, motion).1);
        // **A playing recording, and the same argument once more with one
        // difference** (route B slice ②; §7.44 ③).
        //
        // Every other line above asks whether an *animation* is still running.
        // A video is not an animation this window is running — it is a decoder
        // on another thread producing pictures at its own rate — so the question
        // is not "is a tween moving" but "is there a decoder to collect from".
        // While there is, this window wakes at its own rate and takes whatever
        // has arrived; between arrivals it takes nothing and draws the frame
        // that is standing, which is what `Engine::frame`'s one atomic load is
        // for.
        //
        // **Not gated on reduced motion**, and it is the only line here that is
        // not. `Reduced` is a request about the window's own decoration, not a
        // request for a video to stop moving: a reader who has turned animation
        // off and then pressed play has asked for a recording to play. The
        // *bar's* fade obeys the setting, which is the part that is this
        // window's decoration — see `video_seat::BarSituation::presence`.
        let playing = self.window.video.any_playing() || self.an_animation_is_running();
        // And the bar's own two waits, which do obey it: a bar rising, standing
        // out its dwell, or fading is a reason to wake even when the decoder has
        // nothing new — and no reason at all once it has settled.
        //
        // **A reason to wake and not, by itself, a thing in flight** (review
        // round 3, 2026-09-18). Two of the three clocks folded in there are
        // waits — the intent a still pointer serves out before the bar is
        // offered, and the two seconds a shown bar stands before it begins to go
        // — so the liveness half asks [`video_seat::VideoSeats::bar_is_moving`],
        // which is the fade alone.
        let bar_deadline = self.window.video.bar_deadline(now, motion);
        let bar_moving = self.window.video.bar_is_moving(now, motion);
        // **§7.1.6b′ T-5 — the card column's own, and the one line here that is
        // not about an animation at all.**
        //
        // Every reading above asks whether a tween is still moving. This asks
        // whether a picture is still behind the thing it is a picture of: a pane
        // spoke, and either this tick has not come round yet or the projection it
        // came round for was refused by the throttle. Both are states nothing else
        // in this window would wake the loop out of — the pointer has stopped, the
        // pane's own frame has already been published, and before this line the
        // only thing that came back for the card was a shell that happened to be
        // sending `OSC 133`.
        //
        // **Half of a pair, and the other half is in
        // [`Self::advance_strip_animation`]'s `owes_frame` fold.** This wakes the
        // loop; that lets the woken tick past the early return and on to
        // `refresh_chrome`. Either alone is inert, and the one without the other
        // was measured: a deadline with no fold woke the window every 16 ms to
        // take an early return, so the card stood still *and* the window span.
        //
        // **Not gated on reduced motion**, and for the video's reason rather than
        // its own: a card is not this window's decoration, it is a live picture of
        // a pane, and a reader who asked for stillness asked for tweens to stop
        // rather than for a terminal to stop being drawn. The frame it asks for is
        // the window's own display frame, so a card costs no more than
        // an integrated shell's tab mark always has, and the debt is cleared by
        // the very pass that draws it.
        let cards_behind = self.window.cards.owes_frame();
        // **Everything folded into this line is either a tween, a periodic or a
        // debt this window's own chrome rebuild settles** (review round 3,
        // 2026-09-18) — which is what lets the chrome lane read it as liveness.
        // The two periodics, `page_loading` and `playing`, are their conditions
        // read fresh on this turn and are never latched: the engine is asked
        // whether a navigation is still in flight and the seats are asked
        // whether anything is still decoding, so a page that has landed and a
        // recording that has been paused stop counting on the very next turn.
        // `cards_behind` is a one-shot debt and is settled by `refresh_chrome`,
        // which is what the carry runs, so it cannot survive the frames it asks
        // for.
        let strip_moving = tabs_moving
            || chevron_turning
            || dock_fading
            || cards_moving
            || focus_arriving
            || rail_moving
            || files_turning
            || page_loading
            || passing
            || fading
            || saying
            || disclosing
            || playing
            || cards_behind;
        // **A debt books its own wake** (closure review 2, 2026-09-18, P1). The
        // pictures are serviced above every gate now, so the turn that finds a
        // frame and the turn that may present it are two different turns — and
        // on an otherwise idle window there is nothing else to bring the second
        // one round. A decoder's last picture, or the one a pause leaves
        // standing, would sit in `pictures_owe_a_frame` until some unrelated
        // event happened to wake the loop. It is a **deadline** and never a
        // liveness: nothing is moving, one frame is owed, and this is where the
        // window says when it will pay it — the same shape the pacer's own
        // refusal takes ([`pace::FrameClock::refuse`]).
        let pictures_owe = self.window.pictures_owe_a_frame;
        let pane_moving = self.window.pane_motion.is_animating(now, motion);
        let next_tick = self.strip_animation_next_tick(self.window.frame_clock.interval());
        let paced_strip = strip_moving || pictures_owe;
        let strip_wake = earliest_deadline([
            paced_strip.then_some(next_tick).flatten(),
            rail_wait_deadline,
        ]);
        // The bar's intent and dwell are absolute owner clocks. Its fade and a
        // pane's flight instead ride the strip's absolute tick; their old
        // `now + frame` answers are deliberately not entries in the wake fold.
        let bar_wait = (!bar_moving)
            .then_some(bar_deadline)
            .flatten()
            .map(|deadline| next_tick.map_or(deadline, |tick| deadline.max(tick)));
        AnimationWork {
            deadline: [
                (strip_moving || rail_waiting || pictures_owe)
                    .then_some(strip_wake)
                    .flatten(),
                (bar_moving || pane_moving).then_some(next_tick).flatten(),
                bar_wait,
            ]
            .into_iter()
            .flatten()
            .min(),
            // The two the fold expresses as deadlines of their own, asked here
            // as the predicates they are made of: the panes' own tween, and the
            // bar's fade without the two waits its deadline also carries.
            moving: strip_moving || bar_moving || pane_moving,
        }
    }

    /// The file a Ctrl+click at `hit` may hand to the system viewer (preview matrix §4, "leave this
    /// product"). Never path-*looking* text: the verb has always required that a worker actually
    /// opened and decoded the file, so that a misdetected word cannot launch anything.
    ///
    /// The cells are the hovered pane's own — the very cells wearing the underline that promised
    /// the picture — so the verb and the mark cannot reach different text. Verification is the
    /// scan's `verified` (the decoration worker's record of this file) or the peek's own cache
    /// entry, which is the same worker and the same decoder reached by hovering rather than by
    /// detection.
    pub(crate) fn local_image_path_hit(&self, hit: bt_render::GridHit) -> Option<PathBuf> {
        let (_, leaf, _) = self.hovered_leaf()?;
        let reference = leaf.frame_image_references.at(hit)?;
        (reference.verified
            || matches!(
                self.window
                    .peek_cache
                    .get(&normalized_local_image_path_key(&reference.path)),
                Some(PeekCacheEntry::Ready { .. })
            ))
        .then(|| reference.path.clone())
    }

    /// The verified image reference the pointer is currently standing on, and the pane that draws
    /// it — the cells whose resting dots become a solid underline for as long as the pointer is
    /// there.
    ///
    /// Resolved from that pane's own scan rather than remembered in hover state, because it is a
    /// pure function of "where is the pointer" and "what does that pane draw there", and both can
    /// change without a pointer event: a decode landing turns plain text into an underlined
    /// reference under a pointer that never moved.
    pub(crate) fn hovered_image_reference(&self) -> Option<(SeatId, bt_term::FrameImageReference)> {
        let (seat, leaf, hit) = self.hovered_leaf()?;
        leaf.frame_image_references
            .at(hit)
            .filter(|reference| reference.verified)
            .cloned()
            .map(|reference| (seat, reference))
    }

    /// Repaint when the pointer has moved onto or off a verified reference, so the solid underline
    /// arrives with the pointer rather than with the next unrelated frame.
    ///
    /// The other direction — a decode landing under a pointer that never moved — needs nothing
    /// here: `apply_math_results` already republishes when a completion changed session state, and
    /// the compose step asks the session afresh.
    pub(crate) fn refresh_image_reference_underline(&mut self) -> Result<()> {
        let hovered = self.hovered_image_reference();
        if hovered == self.window.underlined_image_reference {
            return Ok(());
        }
        self.window.underlined_image_reference = hovered;
        self.repaint_hovered_pane()
    }

    /// Record a peek decode outcome and, when the hover is still settled on that path, show the
    /// flyout at the settle pointer.
    pub(crate) fn complete_peek_image(
        &mut self,
        path: PathBuf,
        result: std::result::Result<bt_term::DecodedInlineImage, bt_term::InlineImageDecodeError>,
    ) -> Result<()> {
        let cache_key = normalized_local_image_path_key(&path);
        // **Every picture waiting on this file, not just the lane's.** A decode
        // is addressed by *path* and the answer is the same pixels for whoever
        // asked: a pane, a window torn off it, or both at once. Asking only the
        // texture lane meant a picture in a float was told nothing when its own
        // decode failed and the frame that shows the answer was never owed.
        let waiting: Vec<PreviewSurface> = self
            .preview_picture_hosts()
            .into_iter()
            .filter(|surface| {
                self.preview_picture(*surface).is_some_and(|picture| {
                    normalized_local_image_path_key(&picture.path) == cache_key
                })
            })
            .collect();
        let preview_matches = !waiting.is_empty();
        match result {
            Ok(decoded) => {
                // **And the pages that asked for these pixels to sharpen a
                // picture they are already drawing** (§7.1.3u ③). Owed here,
                // before the pixels go into the store, because this is the one
                // moment they are in hand and named: the page cannot make the
                // exact-size pass itself, which is why it asked for the file at
                // all. See [`owe_sharpened_rasters`] — and note what is *not*
                // done, which is ticking the picture generation: nothing about
                // the document's shape has changed, so no paragraph is re-shaped
                // for a picture that is already the right size on the glass.
                owe_sharpened_rasters(
                    &mut self.window.markdown_pictures,
                    // The one walk ([`documents_held_in`]), spelled with the two
                    // fields it reads rather than through `Self::documents_held`:
                    // the ledger being written is a third field of the same
                    // window, and borrowing it mutably while the holders are read
                    // is only possible when the compiler can see the three are
                    // different fields.
                    documents_pictures(documents_held_in(
                        &self.window.tabs,
                        &self.window.peek_pane,
                    )),
                    &cache_key,
                    &decoded.key,
                    &decoded.rgba,
                    [decoded.width_px, decoded.height_px],
                    Instant::now(),
                );
                self.window.peek_cache.insert(
                    cache_key.clone(),
                    PeekCacheEntry::Ready {
                        key: decoded.key,
                        rgba: decoded.rgba,
                        width_px: decoded.width_px,
                        height_px: decoded.height_px,
                        native_size: decoded.native_size,
                    },
                );
                if let Some(active) = self.window.peek_hover.active.clone()
                    && active.subject.key == cache_key
                {
                    self.show_or_request_peek(&active)?;
                }
            }
            Err(error) => {
                for surface in &waiting {
                    if let Some(picture) = self.preview_picture_mut(*surface) {
                        picture.failure = Some(PictureRefusal::decoded(&error));
                    }
                }
                // **Filed with its reason** (owner's ruling 2026-09-12), so that
                // a surface which opens this file later says the same sentence
                // rather than the one generic line — see [`PeekCacheEntry::Failed`].
                self.window
                    .peek_cache
                    .insert(cache_key.clone(), PeekCacheEntry::Failed(error));
            }
        }
        // **And every markdown page that was waiting to see this file**
        // (§7.1.3k; §7.1.3u's second report). A block that was as tall as its alt
        // text is now as tall as a screenshot, which is a re-flow and not a
        // repaint — so the generation ticks, exactly as a formula's arrival ticks
        // its own.
        //
        // **Waiting, and not merely standing on it.** A page keeps the answer it
        // was given (§7.1.3u), so a decode arriving for a picture the page is
        // already drawing tells it nothing it does not know — and a re-flow is a
        // re-shape of every paragraph on the page. Asked of
        // [`Self::markdown_pictures_awaited`] rather than of every file a page's
        // pictures came from.
        let in_a_page = self
            .markdown_pictures_awaited()
            .iter()
            .any(|file| normalized_local_image_path_key(file) == cache_key);
        if in_a_page {
            self.window.markdown_pictures.generation =
                self.window.markdown_pictures.generation.saturating_add(1);
        }
        if preview_matches || in_a_page {
            self.refresh_preview_for_layout();
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        // A glance card standing over this very file has been drawing an empty
        // ground while the decode was out. Nothing else will move the pointer to
        // rebuild the chrome, so the frame is owed here — and the rebuild is what
        // asks for the resample, which is the next step of the same errand.
        if self.window.file_peek.as_ref().is_some_and(|peek| {
            peek.source
                .file_path()
                .is_some_and(|path| normalized_local_image_path_key(path) == cache_key)
        }) && self.refresh_overlay()
        {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **Take delivery of one video's frame and the facts that came with it** (user ruling
    /// 2026-08-27; §7.23).
    ///
    /// [`Self::complete_peek_image`]'s twin, and the parts that are the same are the same on
    /// purpose: the pixels go into the window's one decode cache under the file's own key, every
    /// surface showing that file is asked to lay out again, and a glance card standing over it is
    /// owed a rebuild because nothing else will move the pointer to ask for one.
    ///
    /// **What differs is that a failure is not a failure.** A picture that will not decode puts a
    /// sentence on the pane — "Preview failed: image could not be loaded" — because a file called
    /// `.png` that is not one is a thing the reader should be told about. A video that will not
    /// decode is very often an ordinary file in a container this machine has no codec for, and the
    /// card and the pane have something true to draw either way: the format and the size. So the
    /// facts are filed **before** the frame is looked at, and the cache is marked `Failed` without
    /// anybody's failure line being written.
    pub(crate) fn complete_peek_video_frame(
        &mut self,
        path: PathBuf,
        glance: VideoGlance,
    ) -> Result<()> {
        let cache_key = normalized_local_image_path_key(&path);
        // Filed first and unconditionally: these outlive the pixels and are the whole of the
        // degraded card — see [`WindowRuntime::video_facts`].
        self.window
            .video_facts
            .insert(cache_key.clone(), glance.facts);
        let entry = match glance.frame {
            Some(frame) => PeekCacheEntry::Ready {
                key: video_frame_texture_key(&path, glance.mtime, frame.width, frame.height),
                rgba: Arc::from(frame.rgba.into_boxed_slice()),
                width_px: frame.width,
                height_px: frame.height,
                // A frame is already fitted into [`VIDEO_FRAME_FIT_PX`] and the
                // recording's own size is filed beside it in `video_facts`,
                // which is where every surface reads it from — see
                // [`PreviewImageState::stated_size`].
                native_size: None,
            },
            // **A container this machine has no codec for, said in the
            // decoder's own words.** The picture lane's variants are about a
            // picture file's size and none of them is this; `Decode` is the
            // seam that carries a reason nobody here can translate, which is
            // what the pane has always printed for a video that would not
            // decode — and it prints nothing at all for one, by the paragraph
            // above.
            None => PeekCacheEntry::Failed(bt_term::InlineImageDecodeError::Decode(
                "no decoder for this container".to_owned(),
            )),
        };
        self.window.peek_cache.insert(cache_key.clone(), entry);
        // Every surface standing on this file, for `complete_peek_image`'s reason: a decode is
        // addressed by path and the answer is the same pixels for a pane, for a window torn off
        // it, or for both at once.
        let waiting = self.preview_picture_hosts().into_iter().any(|surface| {
            self.preview_picture(surface)
                .is_some_and(|picture| normalized_local_image_path_key(&picture.path) == cache_key)
        });
        if waiting {
            self.refresh_preview_for_layout();
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        if self.window.file_peek.as_ref().is_some_and(|peek| {
            peek.source
                .file_path()
                .is_some_and(|path| normalized_local_image_path_key(path) == cache_key)
        }) && self.refresh_overlay()
        {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Take delivery of a preview's display-sized raster.
    ///
    /// **Addressed the way it was asked for — by the picture, never by the
    /// host** (user report, 2026-08-17). A resample is a worker round trip, and
    /// in that time the picture that asked for it may have changed hosts: a pane
    /// torn off into a window, a window docked back into a pane. This used to
    /// hand the answer to whoever held the texture lane *when it landed*, so in
    /// exactly those two moments it was handed to nobody — and the request
    /// ledger it left behind ([`PreviewImageState::pending`]) then made every
    /// later refit answer "already asked", so the picture never came back at
    /// all. The target tuple travels with the pane because it lives on it, which
    /// is why looking the asker up by it is a routing that a move cannot break.
    pub(crate) fn complete_preview_scale(
        &mut self,
        scaled: bt_term::ScaledInlineImage,
    ) -> Result<()> {
        let delivered: PeekThumbnailTarget = (
            scaled.content_key.clone(),
            scaled.width_px,
            scaled.height_px,
        );
        // Every tab's plane, because a float's pane lives with the tab that
        // opened the window and not with the one on screen — and a picture
        // whose window you have since navigated away from is still a picture
        // that asked a question.
        let Some(surface) = self
            .window
            .tabs
            .iter()
            .find_map(|tab| tab.preview_panes.awaiting_scale(&delivered))
        else {
            return Ok(());
        };
        if !self
            .preview_picture_mut(surface)
            .is_some_and(|picture| picture.accept_scaled(scaled))
        {
            return Ok(());
        }
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    pub(crate) fn activate_local_image_path(&self, path: &std::path::Path) {
        let result = native_window(&self.window.window).and_then(|native| {
            bt_platform::open_local_file(native, path)
                .map_err(|error| anyhow!(error))
                .context("open decoded local image in the system viewer")
        });
        if let Err(error) = result {
            eprintln!("recoverable local image open failure: {error:#}");
        }
    }

    /// [`Self::terminal_link_grasp`] on the other surface links are drawn on
    /// (§7.1.5g ⑦), read from the same hover the underline is drawn from and
    /// through the same one expression the press is spent through.
    ///
    /// **It used to be `preview_link_hover.is_some()`**, which is the sentence
    /// the underline makes and not the one the finger makes: a `mailto:` written
    /// into a document wore the pointing hand over a press that did nothing,
    /// which is 7.1.5f's complaint — a mark that answers a hover and not a
    /// press — standing in the preview instead of the terminal.
    ///
    /// A document with no folder resolves no link and therefore answers no
    /// press, which is [`Self::open_preview_link`]'s own first gate said as a
    /// shape.
    pub(crate) fn preview_link_grasp(&self) -> bool {
        let Some((surface, link)) = self.preview_link_hover.as_ref() else {
            return false;
        };
        let Some(document) = self
            .preview_buffer_on(*surface)
            .and_then(|buffer| buffer.source.file_path())
        else {
            return false;
        };
        // [`Self::terminal_link_grasp`]'s modifier, one surface along (§13.45 ①).
        preview_link_answers_a_press(
            input::pointer_chord_held(self.window.modifiers_held),
            &link.target,
            document,
        )
    }

    /// What the pointer is being offered over a picture, if anything (ticket
    /// #60).
    ///
    /// The gesture in flight before the thing under the pointer, exactly as
    /// [`float_grasp`] reads it: a picture carried past its own pane keeps the
    /// closed hand, because the shape changing mid-drag would say something
    /// happened when nothing did (K113, and the mock-up's own line 1710).
    pub(crate) fn image_grasp(&self) -> Option<ImageGrasp> {
        if self.preview_image_drag.is_some() {
            return Some(ImageGrasp::Closed);
        }
        let position = self.window.pointer_position?;
        let (surface, _) = self.preview_surface_at(position)?;
        let (body, image_px) = self.preview_image_geometry(surface)?;
        image_is_pannable(body, image_px, self.preview_image_zoom(surface))
            .then_some(ImageGrasp::Open)
    }

    /// **Start the worker that turns a clipboard picture into a file**
    /// (§7.61, GitHub issue #2).
    ///
    /// Nothing is encoded here. A screenshot off a large display is megabytes of
    /// device-independent bitmap and its PNG encode is tens of milliseconds — on
    /// this thread that is the window not answering, in the middle of the one
    /// gesture a reader is watching. What this does is hand the bytes to a
    /// worker, remember which paste asked, and return; [`Self::adopt_clipboard_picture`]
    /// is the other half.
    ///
    /// **In the workers' band and not below it.** The wallpaper decoder runs at
    /// `BelowNormal` because nobody is waiting for a wallpaper; somebody is
    /// waiting for this, with their hand still on the keyboard.
    pub(crate) fn save_clipboard_picture(
        &mut self,
        target: PasteTarget,
        offered: Vec<bt_platform::PictureBytes>,
    ) -> Result<()> {
        // **A place in the queue before a thread is asked for** (review X-3/X-4).
        // Superseding a job does not stop it — it is inside a decode — so a held
        // `Ctrl+V` starts one per press, each entitled to a picture of its own.
        // The guard is what makes the ceiling on one job a ceiling on the
        // process; it is given up when the worker ends, however it ends.
        let Some(place) = ClipboardPictureJob::take() else {
            eprintln!("clipboard picture workers are all busy; paste ignored");
            return self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::Window,
                None,
                i18n::Text::PasteClipboardPicture.text().to_owned(),
            );
        };
        let generation = self.window.clipboard_picture.withdraw();
        let inbox = self.window.clipboard_picture.inbox();
        let proxy = self.app.event_proxy.clone();
        let folder = clipboard_picture::directory();
        let started = bt_platform::spawn_at_priority(
            "clipboard-picture",
            bt_platform::ThreadPriority::Normal,
            move || {
                // Held for the whole body and dropped with it, on every road out.
                let _place = place;
                let result = clipboard_picture::save(&folder, &offered, SystemTime::now());
                // The generation is compared under the inbox's own lock, so an
                // answer that has been superseded while it was being made is
                // dropped rather than laid on top of the answer that supersedes
                // it (review X-3).
                deliver_clipboard_picture(
                    &inbox,
                    ClipboardPictureAnswer {
                        generation,
                        target,
                        result,
                    },
                );
                // After the answer is in the slot, never before.
                let _ = proxy.send_event(AppEvent::ClipboardPictureReady);
            },
        );
        if started.is_err() {
            // A machine that will not start a thread is a machine that cannot do
            // this paste, and saying nothing would look like a key that did not
            // register. The place goes back with the closure that was never run.
            eprintln!("clipboard picture worker could not be started; paste ignored");
            return self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::Window,
                None,
                i18n::Text::PasteClipboardPicture.text().to_owned(),
            );
        }
        Ok(())
    }

    /// **The file the picture worker wrote, pasted as the path it now is**
    /// (§7.61).
    ///
    /// One line of its own and then [`Self::paste_paths_into`], which is the
    /// whole point: by the time a picture has a file it *is* a path in hand, and
    /// a path in hand is what a drop leaves and what a copy leaves. So the
    /// quoting, the `paste_paths_as` spelling, the refusal notice and the
    /// delivery are the same implementation those two use rather than a third
    /// that drifts — `a_written_picture_is_spelled_exactly_as_a_copied_file_is`
    /// is what goes red if somebody writes one.
    ///
    /// **The recipient and the leading space are therefore read now, not when
    /// the paste was asked for**, because that road reads them itself: they are
    /// facts about the line the bytes are about to land on, and that line has had
    /// the whole of the encode to change.
    ///
    /// **The address is the one the gesture was made at** (review X-1), carried
    /// on the answer rather than re-derived here: a seat is numbered inside its
    /// tab, every new single-pane tab starts at `SeatId(1)`, and resolving a
    /// delayed answer against whichever tab is on top put one tab's screenshot
    /// on another tab's command line. [`Self::live_paste_target`] is where the
    /// three facts are checked.
    ///
    /// A shell that is gone, restarted, or in a tab the reader has left is
    /// dropped in silence, on [`Self::adopt_background_picture`]'s footing: it
    /// answers a gesture that has been superseded, and the file it wrote is
    /// swept by the cap the next paste applies.
    pub(crate) fn adopt_clipboard_picture(&mut self) -> Result<()> {
        let Some(landed) = self.window.clipboard_picture.take_current() else {
            return Ok(());
        };
        let path = match landed.result {
            Ok(path) => path,
            Err(reason) => {
                // `diagnostics::note` and not `eprintln!` (X-7): this is the
                // window thread, and a resident diagnostic it writes must not be
                // able to wait behind whoever is reading a trace.
                diagnostics::note(&format!(
                    "clipboard picture could not be saved; paste ignored: {reason}"
                ));
                return self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::Window,
                    None,
                    i18n::Text::PasteClipboardPicture.text().to_owned(),
                );
            }
        };
        // The same as the clipboard's own road above, for the same reason: the
        // picture was pasted into the pane that had the keyboard, and it still
        // has it.
        self.paste_paths_into(
            landed.target,
            vec![path],
            "write clipboard picture path to PTY",
        )
        .map(drop)
    }

    /// **Take in every picture the shrinker has finished.** Returns whether any
    /// card owes a repaint.
    pub(crate) fn collect_page_pictures(&mut self) -> bool {
        let Some(shrinker) = self.window.web_shrinker.as_ref() else {
            return false;
        };
        let finished = shrinker.collect();
        let mut changed = false;
        for picture in finished {
            changed |= self.window.web_thumbs.settle(picture);
        }
        changed
    }

    pub(crate) fn picture_is_owed(&self) -> bool {
        self.window.pending_frames.pending_frame().is_some()
            || self.window.chrome_present_pending
            || self.window.unpainted_pane_output
            || self.pending_resize_present.is_some()
    }

    pub(crate) fn check_picture_freshness(&mut self, instant: Instant, landed: bool) {
        let owed = self.picture_is_owed();
        let now = present_diagnostics::timestamp(instant);
        let state = &mut self.window.present_diagnostics;
        // A just-completed attempt still owns its pre-call debt for the crossing
        // check, even though the existing caller has already paid that debt.
        let checking_owed = owed || (landed && state.pending_since.is_some());
        if state.pending_since.is_none() {
            state.observe(owed, now, present_diagnostics::progress());
        }
        let shown = self.window.window_shown;
        let minimized = self.window.diagnostic_minimized.get();
        if let Some(line) = state.check(checking_owed, shown, minimized, now, false) {
            hang_watch::during(hang_watch::Station::DiagnosticWrite, || {
                // Native observations are taken only once a line is due. The
                // attention heuristic never decides visibility for this diagnostic.
                let native = native_window(&self.window.window)
                    .ok()
                    .map(bt_platform::native_present_facts)
                    .unwrap_or_default();
                let text = state.line(
                    u64::from(self.window.window.id()),
                    now,
                    present_diagnostics::progress(),
                    line,
                );
                diagnostics::note(&format!(
                    "{text}; {}",
                    present_diagnostics::native_fields(native)
                ));
            });
        }
        if let Some(line) = state.check(checking_owed, shown, minimized, now, landed) {
            hang_watch::during(hang_watch::Station::DiagnosticWrite, || {
                diagnostics::note(&state.line(
                    u64::from(self.window.window.id()),
                    now,
                    present_diagnostics::progress(),
                    line,
                ));
            });
        }
        if !owed && !landed {
            state.observe(false, now, present_diagnostics::progress());
        }
    }

    /// Put the picture that is already on the glass back on the glass, with
    /// whatever the renderer has been told since.
    ///
    /// **No projection, no capture, no composition.** Every frame here is one
    /// this window has already presented; the only things that changed are held
    /// by the renderer — the chrome quads, the caret's blink phase — and by the
    /// pane transforms, which are re-sampled because they are a function of
    /// this instant. See [`chrome_tick_reuses_picture`] for why that is a
    /// complete account of what an animation tick can have moved.
    pub(crate) fn present_retained_picture(&mut self) -> Result<()> {
        let mut attempt = self.begin_present_attempt(FrameSource::Expose, true, true);
        let result = hang_watch::during(hang_watch::Station::RetainedPicture, || {
            self.window.chrome_present_pending = false;
            // **A tab with a shell that has never presented has nothing to keep**,
            // which is what this guard has always said. A tab with *no* shell always
            // has something to keep — its column, its preview, the chrome around
            // them are all retained renderer state — and for it this is not the
            // fallback path but the **only** path: `publish_frame_inner` composes no
            // terminal picture for it and sends it here (§7.1.6h).
            if self.focused().is_some() && self.window.last_presented_frame.is_none() {
                return Ok(());
            }
            let focused_leaf = self.focused_leaf;
            let now = Instant::now();
            // Before the frame is borrowed: this samples the tweens and re-places
            // the preview raster, both of which want the renderer mutably.
            let bodies =
                hang_watch::during(hang_watch::Station::RedrawLayout, || self.pane_draws(now));
            // The same ownership sentence as `redraw`: this path draws the retained
            // frames, so those retained frames are the frames handed to both formula
            // lanes. The guard is two `Option` reads and stands before any lookup,
            // vector build, or allocation.
            let mut math_band_trace = None;
            if self.formula_overlay_is_active() {
                let active = self.window.active_tab;
                let frame_for = |seat| {
                    let body = bodies
                        .iter()
                        .find(|pane| pane.seat == seat)
                        .map(|pane| pane.viewport)
                        .or_else(|| {
                            (seat == focused_leaf).then(|| self.window.renderer.seat_viewport())
                        })?;
                    let frame = if seat == focused_leaf {
                        self.window.last_presented_frame.as_ref()?
                    } else {
                        self.window.tabs[active]
                            .sessions
                            .get(&seat)?
                            .last_presented_frame
                            .as_ref()?
                    };
                    Some((body, frame))
                };
                let trace = self.math_band_trace_for_present(frame_for);
                let placement = hang_watch::during(hang_watch::Station::RedrawOverlay, || {
                    self.math_tool_placement(frame_for)
                });
                let toggle_layers = hang_watch::during(hang_watch::Station::RedrawOverlay, || {
                    self.formula_toggle_layers(now, frame_for)
                });
                hang_watch::during(hang_watch::Station::RedrawOverlay, || {
                    self.refresh_formula_overlay_for_present(now, placement, toggle_layers)
                });
                math_band_trace = self
                    .app
                    .trace_perf
                    .then(|| self.math_band_trace_line(now, trace));
            }
            // The retained picture is the same picture, but the palette and the type size under it may
            // have moved since it was presented — a theme switch re-presents without re-projecting.
            // So the pictures are re-checked here for the same reason the tweens are re-sampled.
            let table_sources = Self::table_sources(
                self.window.last_presented_frame.iter().chain(
                    self.window.tabs[self.window.active_tab]
                        .sessions
                        .values()
                        .filter_map(|leaf| leaf.last_presented_frame.as_ref()),
                ),
            );
            hang_watch::during(hang_watch::Station::RedrawTables, || {
                self.refresh_table_paints(&table_sources)
            });
            let (seat_ids, seat_frames) = Self::retained_seats(
                &self.window.tabs[self.window.active_tab],
                self.window
                    .last_presented_frame
                    .as_ref()
                    .filter(|_| self.focused().is_some()),
                &bodies,
                focused_leaf,
                self.window.renderer.seat_viewport(),
                self.keyboard_owner_is_a_shell(),
            );
            let signature = self.present_signature(&seat_ids, &seat_frames);
            let conditions = self.present_conditions(FrameSource::Expose);
            let trigger = FrameTrigger {
                occurred_at: now,
                source: FrameSource::Expose,
            };
            match Self::present_seats_and_commit(
                &mut self.app.gpu,
                &mut self.window.renderer,
                &self.window.compositor,
                &self.window.window,
                FrameTraces {
                    attempt: &mut attempt,
                    gate: &mut self.window.present_gate,
                    trace_perf: self.app.trace_perf,
                    slot_overwrites: self.window.pending_frames.overwrites(),
                    conditions,
                    preview: &mut self.window.preview_trace_echo,
                    census: &mut self.window.glyph_census_echo,
                },
                &seat_frames,
                PresentIntent { trigger, signature },
            )
            .context("re-present the retained terminal picture")?
            {
                None => Ok(()),
                Some(PresentOutcome::Presented(receipt)) => {
                    self.window.textless_frames = 0;
                    // A picture on the glass is the only proof a device is real, and
                    // it is what closes a device-loss episode — see
                    // [`DeviceLossPilot::a_frame_reached_the_glass`] for the livelock
                    // that stands behind that sentence.
                    self.app.device_loss_pilot.a_frame_reached_the_glass();
                    // **The same line the composed path prints** — see
                    // [`Self::trace_present`] for why it has to be printed here as
                    // well, and for what a tab with no shell looked like to the
                    // instrument while it was not.
                    self.trace_present(trigger.source, receipt, true);
                    self.trace_math_band(math_band_trace);
                    Ok(())
                }
                // The swapchain was not ready. The picture is unchanged and still
                // owed, so the debt is simply re-filed — there is no frame to put
                // back in the slot, which is the whole point of this path.
                //
                // A frame that reached the glass without its characters is owed for
                // the same reason and re-filed the same way: what is on the glass is
                // not the picture this window means to be showing, and the atlas the
                // renderer trimmed on its way out has room for it next time.
                //
                // A window that is not on screen re-files the debt with the rest of
                // them — the picture it owes is unchanged by being invisible — and
                // differs only in that `may_ask_again` will not ask for the turn
                // that pays it. See [`ask_again_after`].
                Some(
                    outcome @ (PresentOutcome::PresentedWithoutText(_)
                    | PresentOutcome::Skipped
                    | PresentOutcome::SkippedNotVisible
                    | PresentOutcome::Reconfigure),
                ) => {
                    self.window.chrome_present_pending = true;
                    if self.window.may_ask_again(&outcome) {
                        hang_watch::during(hang_watch::Station::WindowRedraw, || {
                            self.window.window.request_redraw()
                        });
                    }
                    Ok(())
                }
            }
        });
        self.finish_present_attempt(attempt, &result);
        result
    }
}
