//! `windows` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use super::pointer::touch::let_the_system_translate_touch;
use crate::{
    App, AppEvent, BrokerRelease, Drag, DragHandover, FormulaSwitches, HandoverInto,
    INITIAL_HEIGHT, INITIAL_WIDTH, LaunchPlan, LeafSeed, NewWindowParts, NewWindowPlan,
    PreviewRestore, PtyWakeSignal, RAIL_TRANSITION, RenameExit, RestoredPlacement, RevealTween,
    Runtime, TabSeed, TabState, WindowPosture, WindowRuntime, broker_verdict, create_tab_state,
    dpi_snapshot, dwm_dark_mode_owed, ensure_metrics_match_authoritative_scale,
    ensure_swapchain_matches_inner, first_term_leaf, float, focus_leaf_index, git, hang_watch,
    i18n, ime_outbound, ime_report, install_page_ground_color, install_theme_class_background,
    marks, mouse_trace, native_window, new_window_runtime, opening_window_attributes,
    persisted_preview_pages, persisted_window_bounds, plan_launch, presentation_physical_size,
    preview, preview_source_of_recent, profiles, quit, rail_state_for, recorded_window_placement,
    render_sidebar_mode, render_tab_layout, restore, restore_row_seed, restore_window_placement,
    revive_plan, scrollback_quota, seats, seed, seeded_tab, session_sidebar_mode,
    session_tab_layout, set_option_as_alt, solve_seats, stand_the_window_at, startup_window_rect,
    tear_out_rect, toast, unsaved_line, window_minimum_changed, window_surface_target,
};
use crate::{LeafView, TextScale, owner_door, revived_profile};
use anyhow::Context;
use anyhow::{Result, anyhow};
use bt_layout::{SeatId, SizePolicy, WorkAreaHint};
use bt_persist::{SessionSidebarModeV1, SessionTabLayoutV1, SessionWindowV1, TabV1, WindowStateV1};
use bt_platform::admission::{admitted, doors};
use bt_render::{FrameSource, FrameTrigger, WindowRenderer};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

impl Runtime<'_> {
    pub(crate) fn client_size(&self) -> winit::dpi::PhysicalSize<u32> {
        #[cfg(target_os = "linux")]
        {
            self.window.last_winit_size.unwrap_or_else(|| {
                presentation_physical_size(self.window.renderer.presentation_geometry())
            })
        }
        #[cfg(not(target_os = "linux"))]
        self.window.window.inner_size()
    }

    pub(crate) fn give_foreground_with_retry(&self) -> Result<()> {
        crate::take_owned_keyboard_focus(&self.window.window)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn restore_minimized_window(&self) -> Result<()> {
        crate::restore_minimized_window(&self.window.window)
    }
    pub(crate) fn window_minimized_state(&self) -> Option<bool> {
        #[cfg(target_os = "linux")]
        {
            match crate::linux_window_backend(&self.window.window) {
                Ok(bt_platform::linux_window::Backend::X11) => self.window.native_window_minimized,
                Ok(bt_platform::linux_window::Backend::Wayland) => {
                    self.window.window.is_minimized()
                }
                Err(_) => None,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            native_window(&self.window.window)
                .ok()
                .map(bt_platform::is_window_minimized)
        }
    }

    pub(crate) fn window_maximized_state(&self) -> Option<bool> {
        #[cfg(target_os = "linux")]
        {
            match crate::linux_window_backend(&self.window.window) {
                Ok(bt_platform::linux_window::Backend::X11) => {
                    self.window.maximize_intent.posture_state()
                }
                Ok(bt_platform::linux_window::Backend::Wayland) => {
                    Some(self.window.window.is_maximized())
                }
                Err(_) => None,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Some(self.window.window.is_maximized())
        }
    }

    pub(crate) fn toggle_window_maximized(&mut self) {
        #[cfg(target_os = "linux")]
        {
            match crate::linux_window_backend(&self.window.window) {
                Ok(bt_platform::linux_window::Backend::X11) => {
                    match self.window.maximize_intent.toggle() {
                        crate::WindowMaximizeAction::Request(target) => {
                            self.request_linux_window_maximized(target);
                        }
                        crate::WindowMaximizeAction::WaitForObservation => {
                            if self.window.pending_window_rect.is_none() {
                                self.queue_linux_window_rect_snapshot();
                            }
                        }
                    }
                }
                Ok(bt_platform::linux_window::Backend::Wayland) => self
                    .window
                    .window
                    .set_maximized(!self.window.window.is_maximized()),
                Err(_) => {}
            }
        }
        #[cfg(not(target_os = "linux"))]
        self.window
            .window
            .set_maximized(!self.window.window.is_maximized());
    }

    #[cfg(target_os = "linux")]
    fn request_linux_window_maximized(&mut self, target: bool) {
        self.window.window.set_maximized(target);
        if self.window.pending_window_rect.is_some() {
            self.window.window_rect_refresh_owed = true;
        } else {
            self.queue_linux_window_rect_snapshot();
        }
    }

    pub(in crate::runtime) fn request_window_close(&self) -> Result<()> {
        crate::request_owned_window_close(&self.window.window, &self.app.event_proxy)
    }

    /// **Open a second window on this application** (multiwindow slice C).
    ///
    /// The first window's door reads four files, revives a saved tree, answers a
    /// command line and boots a GPU; this one does none of those, and the
    /// difference is not a shortcut — every one of them is a question about the
    /// *process*, and the process has already answered it. What is left is what
    /// a window is: a native window, the four Win32 bridges that hang off its
    /// handle, a composition tree, a surface on this application's own device,
    /// and one tab.
    ///
    /// **The seed is the default profile's single tab**, which is the same seed
    /// [`Self::new_tab`] plants and the same one a cold launch with no saved
    /// session plants. One sentence about what "new" means, not three.
    ///
    /// **What the plan changes** (multiwindow slice D). `plan.saved` gives the
    /// window tabs to open instead of that one seed, and the two facts a window
    /// wears otherwise follow one rule each:
    ///
    /// * **the rectangle** is the saved one when the *file* asked for this window
    ///   and the product's own size otherwise. A window a verb asked for must not
    ///   open on top of the window that asked — slice C's note, kept — and a
    ///   window drawn out of the vault gets the same answer for the vault's own
    ///   reason: Recent restores places, never layouts.
    /// * **the rail** is `plan.like`'s when a window asked, and the saved
    ///   window's when the file did. Since schema v9 there is no single answer in
    ///   the file to read: the strip and the sidebar are per-window, so the
    ///   honest answer for a new window is the window the reader was looking at.
    pub(crate) fn open_window(
        event_loop: &ActiveEventLoop,
        app: &mut App,
        plan: &NewWindowPlan,
        like: Option<(SessionTabLayoutV1, SessionSidebarModeV1)>,
        _resolved_restore_placement: Option<Option<RestoredPlacement>>,
    ) -> Result<(WindowId, WindowRuntime)> {
        // The default as a seed spells it — the decided row's id, or the unresolved default while
        // the program walk has not answered the rows it reads (see `Runtime::default_profile_id`).
        let default_profile_id = profiles::default_profile_identity(
            &app.settings_store.loaded().default_profile,
            &app.profile_programs,
        );
        // **Where this window opens** (multiwindow slice D). The saved rectangle
        // when the file asked for the window, and the product's own size when a
        // verb did: a second window opened exactly on top of the first is a
        // second window nobody can see, and slice C left that as the open
        // question this slice answers. The judgment is `restore_window_placement`'s
        // in both cases, so a saved rectangle no monitor can see forfeits its
        // corner here exactly as the first window's does.
        #[cfg(target_os = "linux")]
        let placement = _resolved_restore_placement.unwrap_or_else(|| {
            plan.like
                .is_none()
                .then_some(plan.saved.as_deref())
                .flatten()
                .and_then(|saved| restore_window_placement(event_loop, saved))
        });
        #[cfg(not(target_os = "linux"))]
        let placement = plan
            .like
            .is_none()
            .then_some(plan.saved.as_deref())
            .flatten()
            .and_then(|saved| restore_window_placement(event_loop, saved));
        let requested_size = placement.map_or(
            LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT),
            |placement| placement.size,
        );
        let attributes = opening_window_attributes(
            profiles::identity_title(&default_profile_id),
            requested_size,
        );
        let attributes = match placement.and_then(|placement| placement.position) {
            Some(position) => attributes.with_position(position),
            None => attributes,
        };
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("create native window")?,
        );
        let mut ime_report = ime_report::Report::default();
        ime_report.created(ime_report::now_ms());
        ime_report.trace_order(window.id(), "created");
        install_theme_class_background(&window);
        hang_watch::during(hang_watch::Station::ImeAllowed, || {
            ime_outbound::line(|| {
                format!(
                    "window={} {}",
                    u64::from(window.id()),
                    ime_outbound::allowed_line(true, "window_construction")
                )
            });
            window.set_ime_allowed(true)
        });
        ime_report.allowed(true, ime_report::now_ms());
        ime_report.trace_order(window.id(), "allowed");
        // A second window answers the Option key the way the first one does —
        // see that constructor's note. The setting is the process's, and a
        // window that opened before or after it was changed is still a window of
        // this program.
        set_option_as_alt(&window, app.settings_store.loaded().option_sends_alt);
        #[cfg(target_os = "linux")]
        let restored_quake_as_ordinary = plan.quake
            && crate::linux_window_backend(&window)? == bt_platform::linux_window::Backend::Wayland;
        #[cfg(not(target_os = "linux"))]
        let restored_quake_as_ordinary = false;
        let is_quake = plan.quake && !restored_quake_as_ordinary;
        if restored_quake_as_ordinary {
            crate::diagnostics::note(
                "native Wayland cannot restore or summon the saved quake window; opened its saved tabs as an ordinary visible window",
            );
        }
        let native = native_window(&window)?;
        // A second window is a second owner the clipboard may go through — see
        // the first constructor's note.
        bt_platform::register_clipboard_owner(native);
        let custom_window_frame = bt_platform::CustomWindowFrame::install(
            native,
            bt_platform::CustomFrameGeometry {
                title_bar_logical_px: bt_render::WINDOW_TITLE_BAR_LOGICAL_PX as u32,
            },
            // A second window asks for its turn the way the first one does —
            // see that constructor's note (§13.48).
            {
                let proxy = app.event_proxy.clone();
                Box::new(move || {
                    let _ = proxy.send_event(AppEvent::WindowChromeChanged);
                })
            },
        )
        .map_err(|error| anyhow!(error))
        .context("install self-drawn Win32 window frame")?;
        // A second window answers a finger the way the first one does — see
        // that constructor's note.
        let parked_pans = let_the_system_translate_touch(native, &app.event_proxy);
        // **This window's own chrome, read where it was measured** (M3-3;
        // T-MAC-LIGHTS needs it one step earlier than M3-3 did). `install` is
        // where the platform is asked what it still draws in this bar, and the
        // tabs this constructor is about to build are solved against a stage
        // whose top edge that answer decides — so it is read here, once, and
        // handed down. Every reader after the window exists asks the window
        // (`Runtime::platform_chrome`); there is no window to ask yet.
        let platform_chrome = custom_window_frame.platform_chrome();
        let ime_system_caret = bt_platform::ImeSystemCaret::new(native);
        let math_context_menu = bt_platform::MathContextMenu::new(native)
            .map_err(|error| anyhow!(error))
            .context("install deferred formula context menu")?;
        let folder_picker = bt_platform::FolderPicker::new(native)
            .map_err(|error| anyhow!(error))
            .context("install deferred folder chooser")?;
        let image_picker = bt_platform::ImagePicker::new(native)
            .map_err(|error| anyhow!(error))
            .context("install deferred picture chooser")?;
        // **Reported, not propagated** (`the_m1_startup_path_has_no_fatal_platform_call_off_windows`):
        // the export's dialog is not a reason for a window not to open. A window
        // without one says so when `Export…` is pressed.
        let save_picker = bt_platform::SaveFilePicker::new(native)
            .inspect_err(|error| eprintln!("recoverable save dialog install failure: {error}"))
            .ok();
        // **Spike Q5 item 3, paid at last.** `WM_NCCALCSIZE` has just made this
        // window's client area its whole outer rectangle, so what winit built is
        // the size asked for plus a native frame margin this window does not
        // wear. The spike's own second window is the evidence it left: it asked
        // for 720x420 logical and got a 1466x911 client, and a window that skips
        // this line opens one frame margin larger every time. Slice A1 left a
        // note at the first window's copy because there was nowhere else to put
        // the line; this is that somewhere.
        let opened_at = dpi_snapshot(&window, None)?;
        let has_saved_position = placement.is_some();
        // **F5, and it is the *opening* rectangle rather than a move afterwards**
        // (user report 2026-08-27). This used to be stated in `settle_tear_out`,
        // after the window had been dressed, shown and presented **twice** — so
        // the reader watched a window of the product's default size appear at
        // winit's default corner, on top of the window they had just dragged out
        // of, and then jump. Measured on the machine: `BT_DPI stage=show
        // rect=147,147,2067,1347 swapchain_size=1920x1200`, `stage=first-present`
        // at the same rectangle, and only then `BT_TEAR_OUT … rect=1008,980
        // 1100x820` followed by `stage=resized`.
        //
        // Two of the three things the reader reported follow from that ordering
        // alone. The frame carried into the new rectangle was drawn for the old
        // one, so the pane is solved for a stage that is no longer there; and
        // when the tear-out rectangle is the **larger** of the two, the window
        // minus the swapchain is §7.14's L, which the skirt fills with the
        // ground colour — the dark band in the report.
        //
        // Said here, the whole sequence disappears rather than being smoothed
        // over: the window is still hidden (`with_visible(false)`), the
        // compositor and the swapchain are built two statements below, so they
        // are built **at the size the window will keep**. Nothing moves, nothing
        // is resized, and there is no L for a skirt to cover.
        //
        // Both the dpi and the work area are asked of the *point* and not of this
        // window, which is F5's own rule and now literally unavoidable: the
        // window is standing wherever winit put it, which on a two-monitor
        // desktop is very often not the monitor the hand is over.
        let standing = plan
            .receives
            .as_ref()
            .and_then(|errand| {
                #[cfg(target_os = "linux")]
                let ((pointer, grip), (work, dpi)) = errand.at.zip(errand.screen)?;
                #[cfg(not(target_os = "linux"))]
                let (pointer, grip) = errand.at?;
                #[cfg(not(target_os = "linux"))]
                let (work, dpi) = {
                    let dpi = bt_platform::dpi_at(pointer.0, pointer.1);
                    let work = bt_platform::work_area_at(pointer.0, pointer.1)
                        .unwrap_or_else(|_| bt_platform::virtual_screen_rect());
                    (work, dpi)
                };
                let rect = tear_out_rect(pointer, grip, dpi, work);
                eprintln!(
                    "BT_TEAR_OUT pointer={},{} grab={:?} size={:?} dpi={dpi} work={},{} {}x{} rect={},{} {}x{}",
                    pointer.0,
                    pointer.1,
                    grip.grab_logical,
                    grip.size_logical,
                    work.left,
                    work.top,
                    work.right - work.left,
                    work.bottom - work.top,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                );
                Some(rect)
            });
        // Kept, because this rectangle is also the one this window goes into the vault holding —
        // see the `record_window` below.
        let stood_at = standing.unwrap_or_else(|| {
            startup_window_rect(placement, opened_at.rect, opened_at.authoritative_scale)
        });
        let initial_rect = standing.or_else(|| has_saved_position.then_some(stood_at));
        stand_the_window_at(
            &window,
            native,
            stood_at,
            "state the new window's outer rectangle",
        );
        let physical = crate::opening_client_allocation(&window, requested_size, initial_rect);
        let scale_factor = dpi_snapshot(&window, initial_rect)?.authoritative_scale;
        // The visual tree first, because the swapchain hangs off it — §2.3's
        // shape, once per window, because a `Compositor` is parameterised by the
        // HWND it composes above. An owner-thread door (`doors::CompositorBirth`): a refusal is
        // this road's own error.
        let compositor = admitted::<doors::CompositorBirth, _>(|token| {
            bt_platform::Compositor::new(token, native)
        })
        .unwrap_or_else(|refused| Err(refused.to_string()))
        .map_err(|error| anyhow!(error))
        .context("open the window's DirectComposition visual tree")?;
        // Beside the first window's, and for its reason: a second window can be
        // opened straight onto a page (`Move pane to new window` on a web seat).
        install_page_ground_color(&compositor);
        // **This application's device, not a second one** (§2.2). The surface is
        // created by the instance that created the device and its format is
        // asked of the same adapter, which is the whole of the sharing contract;
        // the atlas, both pipelines and the one `FontSystem` come with it, and
        // that last is the saving that is not on the GPU at all.
        //
        // The surface and its configure are one owner-thread door (`doors::SurfaceBirth`); a
        // refusal is this road's own error.
        let mut renderer = admitted::<doors::SurfaceBirth, _>(|token| {
            WindowRenderer::new(
                token,
                &mut app.gpu,
                window_surface_target(&window, &compositor),
                physical.width,
                physical.height,
                scale_factor,
            )
            .map_err(anyhow::Error::from)
        })
        .unwrap_or_else(|refused| Err(anyhow::Error::from(refused)))
        .context("open the new window's surface on this application's device")?;
        let translucency_available = renderer
            .alpha_report()
            .is_some_and(bt_render::SurfaceAlphaReport::is_premultiplied);
        ensure_metrics_match_authoritative_scale(renderer.scale_factor(), scale_factor)?;
        ensure_swapchain_matches_inner(&renderer, physical)?;
        let render_physical = presentation_physical_size(renderer.presentation_geometry());
        let pty_wake = PtyWakeSignal::new(app.event_proxy.clone());
        let wake = &pty_wake;
        // **The rail this window opens wearing** (multiwindow slice D). The strip
        // and the sidebar became per-window facts with schema v9, so there is no
        // longer one answer in the file to read: a window the file described
        // wears its own, and a window a verb asked for wears the rail of the
        // window that asked. §2.4 rule 3 still holds — the setting is worn one
        // window at a time — it is only that "one window at a time" now has more
        // than one answer to be.
        let (tab_layout, sidebar_mode) = like
            .or_else(|| {
                plan.saved
                    .as_deref()
                    .map(|saved| (saved.tab_layout, saved.sidebar_mode))
            })
            .unwrap_or_default();
        let rail = rail_state_for(
            render_tab_layout(tab_layout),
            render_sidebar_mode(sidebar_mode),
        );
        // The posture this window opens in, which is that pair joined to the focus
        // bit — the state the two solves below are entitled to, for the reason the
        // first window's own opening solve states.
        let opening_rail = seats::RailState {
            focus: app.settings_store.loaded().focus_mode,
            ..rail
        };
        // **What this window opens holding.** The launch's own split, run over
        // this window's saved tabs (§7.1.4 read one level up): a pinned tab is an
        // answer already given and opens, and the rest is the prompt's to ask
        // about — but only when the launch is what queued this window. A window
        // opened because the reader said "Restore", or pressed a Recent row, is
        // itself the answer, so everything in it opens.
        let saved = plan.saved.as_deref();
        let plan_launched = saved.map(|saved| {
            if plan.ask_about_unpinned {
                plan_launch(&saved.tabs, saved.active_tab as usize, false)
            } else {
                LaunchPlan {
                    open: saved.tabs.clone(),
                    ask: Vec::new(),
                    active_open: Some(saved.active_tab as usize),
                    placeholder: false,
                }
            }
        });
        let roots: Vec<_> = plan_launched
            .as_ref()
            .filter(|plan| !plan.open.is_empty())
            .map(|plan| plan.open.iter().map(revive_plan).collect())
            .unwrap_or_else(|| {
                vec![(
                    // **A window opened to *receive* a tab opens no shell** (user
                    // report 2026-08-27, and the whole of the stall in it).
                    //
                    // The stand-in is scaffolding: it exists because a window has
                    // to hold a tab between being built and being handed the one
                    // it was opened for, and `retire_the_stand_in` takes it away
                    // in the same turn. Until now it was a *terminal*, so every
                    // tear-out paid a `CreatePseudoConsole` + `CreateProcess` for
                    // a shell nobody would ever look at, and then paid
                    // `PtySession::shutdown` to kill it — `child.kill()`,
                    // `child.wait()`, `reader.join()`, all three on the window
                    // thread. Measured on the machine, one tear-out:
                    // `spawn=31.4ms`, `transfer=28.3ms`, **`retire=2922.2ms`** —
                    // three seconds of frozen loop for a shell that was two
                    // hundred milliseconds old.
                    //
                    // `SeatKind::Placeholder` is this program's own word for a
                    // leaf with nothing in it (`LeafNodeV1::Unknown`), and it is
                    // the honest shape here: `create_tab_state` walks
                    // `seats.terminals()` to open sessions, a placeholder is not
                    // one, so the tab costs nothing to make and nothing to
                    // destroy. Nobody ever sees it — the transfer and the
                    // retirement both run before this window is shown (see
                    // [`FolioApp::open_pending_window`]) — and if the transfer is
                    // refused the window is closed without ever having been on
                    // the glass.
                    //
                    // Only for a receiving plan: every other door opens a window
                    // the reader is going to work in, and that window's first tab
                    // is a shell.
                    if plan.receives.is_some() {
                        seats::Seats::lone_seat(&bt_layout::Seat::new(
                            SeatId(1),
                            bt_layout::SeatKind::Placeholder,
                        ))
                        .0
                    } else {
                        seats::Seats::lone_terminal()
                    },
                    TabSeed::default(),
                    BTreeMap::new(),
                    BTreeMap::new(),
                    PreviewRestore::default(),
                )]
            });
        let mut tabs = Vec::with_capacity(roots.len());
        for (seats, seed, leaves, files, preview) in roots {
            let (tab, _) = create_tab_state(
                app.tab_ids.mint(),
                seats,
                LeafView::at(&mut app.gpu, &renderer, TextScale::ACTUAL)?,
                &renderer,
                render_physical,
                wake,
                None,
                &leaves,
                &files,
                &preview,
                seed,
                &app.profile_programs,
                &default_profile_id,
                &app.settings_store.loaded().default_profile,
                // The opening rectangle is this program's, exactly as the first
                // window's is: nobody has taken hold of a frame that has not been
                // shown yet.
                SizePolicy::Lawful,
                opening_rail,
                platform_chrome,
                FormulaSwitches::from_settings(app.settings_store.loaded()),
                scrollback_quota(app.settings_store.loaded().scrollback_lines),
                app.settings_store.loaded().line_wrapping,
            )?;
            tabs.push(tab);
        }
        let active_tab = plan_launched
            .as_ref()
            .and_then(|plan| plan.active_open)
            .unwrap_or(0)
            .min(tabs.len() - 1);
        let pending_restore = plan_launched
            .as_ref()
            .map(|plan| plan.ask.clone())
            .unwrap_or_default();
        // **Which tab, and not which number** — read off the tab that was built
        // rather than written as `TabId(1)`, which stopped being this window's
        // first tab the day the numbering became the application's (F1b).
        let placeholder_tab = plan_launched
            .as_ref()
            .is_some_and(|plan| plan.placeholder)
            .then(|| tabs[0].id);
        let (_, _, terminal_seat, seat_viewport) = solve_seats(
            &tabs[active_tab].seats,
            &renderer,
            render_physical,
            SizePolicy::Lawful,
            opening_rail,
            platform_chrome,
        );
        renderer.set_seat_viewport(terminal_seat);
        let maximized = placement.is_some_and(|placement| placement.maximized);
        let id = window.id();
        // **Before the window is dressed, because dressing reads it** (§7.54).
        // `is_quake` was decided from this window's actual native backend: a
        // saved Wayland summon stays an ordinary visible window because the
        // compositor cannot provide the restore/focus lifecycle.
        if is_quake {
            app.quake.adopt(id);
        }
        // **This window's own rectangle is in the vault before anything can ask it for one.**
        //
        // [`Runtime::window_snapshot`] measures a rectangle only while a window is *normal*; the
        // other two postures fall back to what this window last said about itself, and a window
        // that had never said anything fell back to `WindowStateV1::default()` — the product's
        // 100,100,1280,800 placeholder, which is the right answer for "there was no prior session
        // at all" and a wrong one for "this window has not been photographed yet".
        //
        // A **second window restored maximized** is exactly that case: it is maximized from the
        // moment it is shown, so its first snapshot has no measured rectangle, and the corner and
        // extent the file recorded for it were replaced by the placeholder on the first launch
        // after it. The first window never had this because `FolioApp::resumed` seeds its picture
        // with the paragraph it was opened from (`window_pictures: vec![(window.id(), …)]`); this
        // is that same seeding said at the other door, in the one form this door can say it —
        // the rectangle it is standing at, which for a restored window *is* the saved one.
        //
        // The tabs are filled in a few lines below, by `mark_session_dirty` on a runtime that has
        // them; an entry with none is momentary and, were a write to catch it, `plan_windows`
        // drops a window with no tabs on the way back in.
        #[cfg(target_os = "linux")]
        let opening_bounds = initial_rect
            .map(|rect| persisted_window_bounds(rect, scale_factor))
            .or_else(|| plan.saved.as_ref().map(|saved| saved.placement.bounds))
            .unwrap_or_else(|| WindowStateV1::default().bounds);
        #[cfg(not(target_os = "linux"))]
        let opening_bounds = persisted_window_bounds(stood_at, scale_factor);
        app.record_window(
            id,
            SessionWindowV1 {
                placement: WindowStateV1 {
                    bounds: opening_bounds,
                    dpi: renderer.dpi_milli().get(),
                    maximized,
                    monitor_id: None,
                },
                ..SessionWindowV1::default()
            },
            Instant::now(),
        );
        let mut window = new_window_runtime(NewWindowParts {
            ime_report,
            parked_pans,
            favicons: Rc::clone(&app.favicons),
            renderer,
            tabs,
            active_tab,
            pty_wake,
            translucency_available,
            custom_window_frame,
            compositor,
            window,
            math_context_menu,
            folder_picker,
            image_picker,
            save_picker,
            ime_system_caret,
            rail,
            seat_viewport,
            motion: app.motion,
            event_proxy: app.event_proxy.clone(),
            // **The question stays with the window it is about**, so answering it
            // once puts each row back where it came from. The card itself is
            // raised in one window and once per process — see
            // [`App::restore_question`].
            pending_restore,
            // **Never here** (ruling ①). One process asks once, and the window
            // that asks is the one the process opened with; a window that opens
            // afterwards carries its own share of the question and raises no card
            // of its own.
            raise_restore_prompt: false,
            // Only a shell opened *because there was no answer* is scaffolding —
            // the launch's own rule, kept here because this door now takes the
            // same plan the launch does.
            placeholder_tab,
        });
        window.restored_quake_as_ordinary = restored_quake_as_ordinary;
        window.focus_mode = app.settings_store.loaded().focus_mode;
        window.focus_reveal =
            RevealTween::resting(f32::from(u8::from(window.focus_mode)), RAIL_TRANSITION);
        let mut runtime = Runtime {
            app,
            window: &mut window,
        };
        runtime.dress_new_window(native)?;
        // The seed above with this window's tabs written into it, taken here rather than left to
        // the caller so that no turn of the loop can find a paragraph with nothing in it. The
        // fallback it reads is the seed, which is what makes a maximized window's first snapshot
        // carry the rectangle it would unmaximize to instead of the placeholder.
        runtime.mark_session_dirty(Instant::now());
        // **A window that is about to be handed a tab is not shown holding the
        // stand-in** (user report 2026-08-27).
        //
        // Showing is two presents and a `ShowWindow` ([`Runtime::show_new_window`]),
        // and until now both presents happened here — with the scaffolding tab on
        // the stage and, before the line above was written, at the wrong
        // rectangle as well. What the reader saw was the wrong window, and then
        // the right one; F1c's own note two doors up ("the window is standing and
        // has not drawn a frame, so the stand-in tab it opened holding is never
        // seen") had been false since the day it was written.
        //
        // So the receiving door shows the window itself, once the tab has
        // arrived and the stand-in has gone — see
        // [`FolioApp::open_pending_window`]. The window stays hidden in between,
        // which is exactly the state `with_visible(false)` opened it in, and a
        // transfer that is *refused* closes it without it ever having been on the
        // glass.
        // **And an X11 window a key summons is not shown by its door either**
        // (§7.54): it is born hidden and stays that way until the key press. A
        // saved Wayland summon has `is_quake == false` above, so its saved tabs
        // are shown as ordinary UI instead of becoming an un-restorable hidden
        // window. See `FolioApp::settle_quake`.
        if plan.receives.is_none() && !is_quake {
            // Maximized only if the file said this window was: a window a verb
            // asked for is one nobody has told to be.
            runtime.show_new_window(maximized)?;
            // The pages of the tabs this window opened holding, on the launch
            // door's own terms and for its reason.
            runtime.revive_all_web_pages()?;
        }
        Ok((id, window))
    }

    /// Everything a window owes itself between "its surface exists" and "it can
    /// be shown", shared by both doors that open one.
    ///
    /// The ground and the two postures are read from settings that belong to the
    /// application, and that is exactly why they are said once *per window*:
    /// §2.4's third rule — the setting changes every window, but the sentence
    /// said to DWM is one window's.
    pub(crate) fn dress_new_window(&mut self, native: bt_platform::NativeWindow) -> Result<()> {
        self.refresh_work_area();
        // **The window's ground, before the first frame** (§7.1.6c-4b). The
        // clear carries the ground's alpha, so a window that put this on after
        // its first present would flash opaque; the picture is decoded here for
        // the same reason. A missing or unreadable file costs a card and an
        // ordinary window, never a failed launch.
        self.reload_background_picture()?;
        self.apply_window_ground()?;
        // The two postures, re-applied because they are postures: a window that
        // was told to stay in front last week is a window that stays in front
        // today. Both are best-effort — neither is a reason to refuse to open —
        // and the acrylic call is skipped outright on a Windows that has no such
        // attribute, which is also what greys its row.
        // **The posture is read off the window and not off the settings file**
        // (§7.54). A summoned terminal is above every other window because that
        // is the whole of what summoning it means — a strip that came down
        // *behind* the editor it was called over would be a strip nobody can see
        // — so the row does not get a vote on this one window. Every other window
        // reads the row, exactly as it always did.
        if (self.is_quake_window() || self.app.settings_store.loaded().always_on_top)
            && let Err(error) = bt_platform::set_window_topmost(native, true)
        {
            eprintln!("recoverable always-on-top failure: {error}");
        }
        // Before the backdrop and not after it, though the measurement says
        // either would do: this is also the border's colour, and a window that
        // wore a light border for its first frame would have flickered.
        self.apply_window_dark_mode()?;
        if self.app.acrylic_available
            && self.app.settings_store.loaded().acrylic
            && let Err(error) = bt_platform::set_system_backdrop(native, true)
        {
            eprintln!("recoverable system backdrop failure: {error}");
        }
        // **The pinned tabs' preview panes ask for their files here**, and this
        // is the earliest they can: the worker is a field of the runtime, so
        // there is nothing to ask until the struct above exists. Every other
        // revive door (`answer_restore`, `reopen_recent`) asks the moment it
        // pushes its tab; this is the launch door doing the same, and without it
        // a restored pane sits on "Loading …" forever — measured on the real
        // machine, which is also why it is a loop over every tab rather than
        // over the active one.
        for index in 0..self.window.tabs.len() {
            self.request_revived_previews(index);
        }
        self.apply_window_min_inner_size()?;
        // **The platform's own buttons, put on the band this window is opening
        // with** (T-MAC-LIGHTS x T-MAC-PILL). `CustomWindowFrame::install` was
        // handed Folio's own bar height, because that is the one number a frame
        // knows before anything has measured the platform's; a window opening
        // into a layout that wears the platform's band instead is corrected
        // here — in the step both doors that open a window take, and before the
        // window is on the glass, so no frame is ever drawn with the lights on
        // the wrong axis.
        self.follow_the_window_band()?;
        // **Where the window is, before anything reads it** (ticket 48): the install road drains
        // this window's shells before its first turn, and that drain reads the turn's reading.
        self.observe_window_place();
        // **Written at once, not on the first turn** (ticket 49): a window must not
        // reach the taskbar untitled, and nothing has been sent yet, so the
        // throttle has no interval to hold it for.
        self.want_title();
        self.flush_title(Instant::now());
        self.refresh_chrome();
        Ok(())
    }

    /// **Say which title this window wants** (ticket 49; `docs/ARCHITECTURE.md`
    /// §5.3 row 14).
    ///
    /// The active tab's display title, handed to [`WindowRuntime::title`]. Every
    /// road that can change it — output renaming a tab, a tab switch, a rename
    /// finished, a held-back screen released, a window born — calls this and
    /// nothing else; none of them tells the OS. [`Self::flush_title`] does, once
    /// a turn.
    pub(crate) fn want_title(&mut self) {
        let title = self.display_title();
        self.window.title.want(title);
    }

    /// **The one call to `Window::set_title` in this program** (ticket 49).
    ///
    /// The wanted title reaches the OS only when it differs from the one last
    /// written, and at most once a display frame ([`crate::pace::FrameClock::interval`]);
    /// a title held back is written when its deadline comes, which the turn
    /// folds into its wake-up, so the last one always lands. The interval is the
    /// frame's rather than "once per presented frame" because a minimised or
    /// cloaked window presents nothing, and its taskbar button and Alt+Tab entry
    /// still show the title.
    ///
    /// Stays on this thread (§5.2: window title is native affinity). What this
    /// removes is the repetition; one write that a real change needs can still
    /// wait on the shell, and the station says so when it does.
    pub(crate) fn flush_title(&mut self, now: Instant) {
        let interval = self.window.frame_clock.interval();
        let Some(title) = self.window.title.take_due(interval, now) else {
            return;
        };
        // An owner-thread door (`doors::TitleFlush`, whose station the meter enters). A refusal
        // keeps the title wanted, and the next turn writes it.
        let window = &self.window.window;
        if admitted::<doors::TitleFlush, _>(|token| owner_door::set_title(token, window, &title))
            .is_err()
        {
            self.window.title.refused(title);
        }
    }

    /// Put the window on the screen — the other half of
    /// [`Self::dress_new_window`], with the launch trace between them.
    pub(crate) fn show_new_window(&mut self, maximized: bool) -> Result<()> {
        self.put_the_window_on_the_glass(maximized)
    }

    /// The three things showing a window does, said once for the two doors that
    /// do it.
    pub(in crate::runtime) fn put_the_window_on_the_glass(
        &mut self,
        maximized: bool,
    ) -> Result<()> {
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        // A hidden surface can either accept the clear or report Occluded. In the latter case
        // redraw() republishes the frame; the second call presents immediately after ShowWindow.
        self.redraw()?;
        // Maximize before the window is shown, not after: `SW_MAXIMIZE` reveals a
        // hidden window itself, so this is one transition into the state the
        // session recorded rather than a normal-sized window that jumps. The
        // normal rectangle set above survives as the placement Windows restores
        // the window to when the user unmaximizes it.
        if maximized {
            #[cfg(target_os = "linux")]
            {
                match crate::linux_window_backend(&self.window.window) {
                    Ok(bt_platform::linux_window::Backend::X11) => {
                        self.window.maximize_intent.request_initial(true);
                        self.request_linux_window_maximized(true);
                    }
                    Ok(bt_platform::linux_window::Backend::Wayland) | Err(_) => {
                        self.window.window.set_maximized(true);
                    }
                }
            }
            #[cfg(not(target_os = "linux"))]
            self.window.window.set_maximized(true);
        }
        // An owner-thread door (`doors::SetVisible`, whose station the meter enters). A refusal
        // leaves the window hidden and takes the show road's own failure.
        admitted::<doors::SetVisible, _>(|token| {
            owner_door::set_visible(token, &self.window.window, true);
        })?;
        self.window.ime_report.shown(ime_report::now_ms());
        self.window
            .ime_report
            .trace_order(self.window.window.id(), "shown");
        self.window.window_shown = true;
        #[cfg(target_os = "linux")]
        self.queue_linux_window_rect_snapshot();
        // Showing a hidden Win32 window can synchronously settle it onto a different monitor.
        // Query Win32 directly: winit's cached scale can race during initial monitor placement.
        self.reconcile_authoritative_dpi("show")?;
        // Force one presentation after ShowWindow so the first-present reconciliation below is a
        // visible startup stage even when the hidden pre-clear already succeeded.
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        self.redraw()
    }

    /// **This window is the one the reader is in now** (§7.59).
    ///
    /// Moved to the back rather than appended, so the list stays a history with
    /// one entry per window instead of a log with one entry per visit: a reader
    /// alternating between two windows all afternoon would otherwise grow a
    /// vector for as long as the process runs.
    ///
    /// **The summoned terminal writes itself down like any other window**, and
    /// is filtered out where the list is *read* ([`most_recently_active_window`])
    /// rather than where it is written. One exclusion, at the one door that
    /// cares, is a rule that cannot be half-applied — and the fact this records
    /// is true of it: the reader really was in it.
    pub(crate) fn note_this_window_was_visited(&mut self) {
        let id = self.window.window.id();
        self.app.activated.retain(|visited| *visited != id);
        self.app.activated.push(id);
    }

    /// Open a Recent entry as a new tab — the door Recent and Ctrl+Shift+T share.
    ///
    /// Index 0 is "the one I just closed", which is the whole of what undo-close
    /// is: not a separate store, just the front of this one.
    pub(in crate::runtime) fn reopen_recent(&mut self, index: usize) -> Result<()> {
        let Some(entry) = self.app.recent.take(index) else {
            return Ok(());
        };
        let mut pages = entry.previews;
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        let wake = &self.window.pty_wake;
        let id = self.app.tab_ids.mint();
        // **The one seat the entry names, whichever of the three shapes it is**
        // (§7.1.6h). Everything after this point is shape-blind: the pages, the
        // pins, the `create_tab_state` call and the pin ruling are the same three
        // dozen lines for a shell, a folder and a file, which is the whole reason
        // the shapes are resolved into a seat here rather than into three copies
        // of this function.
        //
        // The `Term` arm used to be the only one, with a `return` under it whose
        // note read "a files place has no shell to start; the pane that would
        // host it is T5's". T5 landed.
        let (mut seats, manual_name, leaf_seed, files) = match entry.seed {
            seed::Seed::Term {
                profile_id,
                cwd,
                manual_name,
            } => {
                let seats = seats::Seats::lone_terminal();
                // The seed's own profile, never the default one — H66's contract
                // read for a Recent row: the shell you are asking back is the
                // shell you had, and "whatever the default is today" is a
                // different tab wearing this one's folder.
                // The saved id, or the fallback's when this build has no such row —
                // `revive_plan`'s own reading, for the same reason.
                let (profile, unknown_profile_id) = revived_profile(&profile_id);
                let leaves = BTreeMap::from([(
                    seats.identity(),
                    LeafSeed {
                        profile,
                        // Whether the folder still stands is asked by the pane's birth
                        // (`profiles::BirthPlace`), never this thread.
                        cwd: Some(cwd)
                            .filter(|cwd| !cwd.is_empty())
                            .map(|cwd| profiles::SeedPlace::Carried(PathBuf::from(cwd))),
                        unknown_profile_id,
                        card_skip: 0,
                        prefill: None,
                        carried_environment: None,
                    },
                )]);
                (seats, manual_name, leaves, BTreeMap::new())
            }
            // A folder tab comes back as the column it was, rooted where it was
            // rooted, at the width a fresh column gets: the vault stores a place
            // and not a layout (§7.1.4), so a remembered width would be the one
            // promise Recent has always declined to make.
            seed::Seed::Files { root } => {
                let (seats, seat) = seats::Seats::lone_seat(
                    &bt_layout::Seat::new(bt_layout::SeatId(1), bt_layout::SeatKind::Files)
                        .with_fixed_extent(bt_layout::FILES_W),
                );
                let files = BTreeMap::from([(
                    seat,
                    seats::FilesLeafState {
                        root,
                        ..seats::FilesLeafState::default()
                    },
                )]);
                (seats, None, BTreeMap::new(), files)
            }
            // And a file tab as the preview it was, on the file it was on.
            //
            // **The seed's path replaces the page list rather than joining it.**
            // The two say the same thing — `preview_pages` reads the tab's
            // preview panes, and a file tab has exactly the one — so appending
            // would reopen a tab with the same file on two panes. The *seed* is
            // the authority because it is what identifies the tab; `previews` is
            // a list of what its panes were showing, which for this shape is a
            // longer way of saying the same word.
            seed::Seed::Preview { path, source } => {
                let (seats, _) = seats::Seats::lone_seat(&bt_layout::Seat::new(
                    bt_layout::SeatId(1),
                    bt_layout::SeatKind::Preview,
                ));
                pages = vec![match source {
                    bt_persist::PreviewSourceV1::File => bt_persist::RecentPreviewV1::File(path),
                    bt_persist::PreviewSourceV1::Url => {
                        bt_persist::RecentPreviewV1::Page { url: path }
                    }
                }];
                (seats, None, BTreeMap::new(), BTreeMap::new())
            }
            // **A window comes back as a window, not as a tab** (multiwindow
            // slice D, ruling ②), so this is the one shape that leaves by
            // another door. Recorded rather than opened, for
            // [`App::pending_new_windows`]'s standing reason — opening a window
            // needs the `ActiveEventLoop`, which a `Runtime` has never held —
            // and the tabs are handed over in the *file's* vocabulary, so that
            // the window is revived by `revive_plan`: the one function a pinned
            // tab at launch, a Restore, a Recent row and Ctrl+Shift+T all
            // already go through.
            seed::Seed::Window { seeds } => {
                let tabs = seeds.iter().map(seeded_tab).collect();
                let id = self.window.window.id();
                self.app
                    .pending_new_windows
                    .push(NewWindowPlan::revived(id, tabs));
                self.mark_session_dirty(Instant::now());
                return Ok(());
            }
        };
        // **The pages come back beside the shell** (裁决 10). Each one lands
        // through the same `add_preview` an open goes through, so the address
        // rule (§7.1.3's far-right seat) is the rule and not a second copy of it
        // — and each pane but the last is pinned on the way past, because a
        // pinned pane is what stops the next file reusing the seat instead of
        // opening beside it (P95). The tab therefore comes back with as many
        // preview panes as it was closed with, and with exactly one of them
        // still the reuse target, which is the arrangement §7.1.3 requires of
        // any tab at rest.
        //
        // Pins are not read back from the entry because they were never written
        // there: Recent restores the places you were, not a layout.
        //
        // **A preview seat the shape already stood up is used before a new one is
        // added** (§7.1.6h). A file tab arrives here holding exactly the pane its
        // page belongs on; calling `add_preview` for it would open a second pane
        // beside the empty one and leave the tab showing its file in the wrong
        // half of itself. Every other shape arrives with no preview seat at all,
        // so the list is empty and the loop is the loop it always was.
        let metrics = self.seat_metrics();
        let mut standing: std::collections::VecDeque<SeatId> =
            seats.preview_seats().into_iter().collect();
        let mut preview_cur: BTreeMap<SeatId, preview::PreviewSource> = BTreeMap::new();
        for (page, is_last) in pages
            .iter()
            .enumerate()
            .map(|(i, page)| (page, i + 1 == pages.len()))
        {
            let seat = match standing.pop_front() {
                Some(seat) => seat,
                None => match seats.add_preview(&metrics) {
                    Some(seat) => seat,
                    None => break,
                },
            };
            preview_cur.insert(seat, preview_source_of_recent(page));
            if !is_last {
                seats.toggle_preview_lock(seat);
            }
        }
        let preview = PreviewRestore::from_pages(preview_cur);
        let born = LeafView::at(&mut self.app.gpu, &self.window.renderer, TextScale::ACTUAL)?;
        let (tab, _) = create_tab_state(
            id,
            seats,
            born,
            &self.window.renderer,
            render_physical,
            wake,
            None,
            &leaf_seed,
            &files,
            &preview,
            TabSeed {
                manual_name,
                // A reopened tab is not pinned: it is coming back because you
                // asked for it now, which is not the same as promising to bring
                // it back every time.
                pinned: false,
                // A new tab owns no launch's environment.
                carried_environment: None,
            },
            &self.app.profile_programs,
            &self.default_profile_id(),
            &self.app.settings_store.loaded().default_profile,
            self.window.size_policy,
            // The posture, for [`Self::resolve_seat_layout`]'s reason.
            self.rail_posture(),
            self.platform_chrome(),
            FormulaSwitches::from_settings(self.app.settings_store.loaded()),
            scrollback_quota(self.app.settings_store.loaded().scrollback_lines),
            self.app.settings_store.loaded().line_wrapping,
        )?;
        // Appended, which keeps the pinned run intact without a re-sort: a new
        // unpinned tab belongs at the end by construction.
        self.window.tabs.push(tab);
        self.request_revived_previews(self.window.tabs.len() - 1);
        self.revive_web_pages(self.window.tabs.len() - 1)?;
        self.apply_window_min_inner_size()?;
        self.activate_tab(self.window.tabs.len() - 1, true)
    }

    /// Answer the restore prompt.
    ///
    /// Restoring **appends** — the pinned tabs are already standing and are not
    /// up for discussion (mock-up 7492-7496). Declining keeps whatever you are
    /// looking at, which is why the button says "No thanks" rather than "Start
    /// fresh": fresh is already on the screen.
    ///
    /// Declining is not discarding. The tabs you did not take back go into the
    /// vault, so the door you did not walk through is still there — Ctrl+Shift+T
    /// and the Recent list can both still reach them. Nothing a user had open is
    /// ever dropped on the floor by a single click.
    pub(crate) fn answer_restore(&mut self, restore: bool) -> Result<()> {
        let pending = std::mem::take(&mut self.window.pending_restore);
        if pending.is_empty() {
            return Ok(());
        }
        if !restore {
            let now = SystemTime::now();
            for tab in &pending {
                if let Some(leaf) = first_term_leaf(&tab.root) {
                    self.app.recent.record(
                        seed::Seed::Term {
                            profile_id: leaf.profile_id.clone(),
                            cwd: leaf.cwd.clone(),
                            manual_name: leaf.manual_name.clone(),
                        },
                        // Declining is not discarding, so the entry has to carry
                        // everything taking the tab back would have brought —
                        // including the page it was showing (裁决 10).
                        persisted_preview_pages(tab),
                        now,
                    );
                }
            }
            self.mark_session_dirty(Instant::now());
            return Ok(());
        }
        let render_physical =
            presentation_physical_size(self.window.renderer.presentation_geometry());
        // The placeholder existed only because we had no answer; now we do. It
        // goes only if it is untouched — a shell you have already typed into is
        // yours, not scaffolding.
        let placeholder = self.window.placeholder_tab.take();
        let first_revived = self.window.tabs.len();
        for tab in &pending {
            let (seats, seed, leaves, files, preview) = revive_plan(tab);
            let wake = &self.window.pty_wake;
            let id = self.app.tab_ids.mint();
            let born = LeafView::at(&mut self.app.gpu, &self.window.renderer, TextScale::ACTUAL)?;
            let (revived, _) = create_tab_state(
                id,
                seats,
                born,
                &self.window.renderer,
                render_physical,
                wake,
                None,
                &leaves,
                &files,
                &preview,
                seed,
                &self.app.profile_programs,
                &self.default_profile_id(),
                &self.app.settings_store.loaded().default_profile,
                self.window.size_policy,
                // The posture, for [`Self::resolve_seat_layout`]'s reason.
                self.rail_posture(),
                self.platform_chrome(),
                FormulaSwitches::from_settings(self.app.settings_store.loaded()),
                scrollback_quota(self.app.settings_store.loaded().scrollback_lines),
                self.app.settings_store.loaded().line_wrapping,
            )?;
            self.window.tabs.push(revived);
            self.request_revived_previews(self.window.tabs.len() - 1);
            self.revive_web_pages(self.window.tabs.len() - 1)?;
        }
        if let Some(placeholder) = placeholder
            && self.window.tabs.len() > 1
            && let Some(index) = self
                .window
                .tabs
                .iter()
                .position(|tab| tab.id == placeholder)
        {
            let mut removed = self.window.tabs.remove(index);
            // The same leak `close_tab` carried, on the path that retires the
            // launch placeholder once the session file's own tabs are standing.
            // A placeholder holds one shell today and always has; asking for all
            // of them costs nothing and stops this being the copy that is still
            // wrong the day it holds two.
            removed.retire_all_shells();
        }
        self.apply_window_min_inner_size()?;
        let landing = first_revived.saturating_sub(usize::from(placeholder.is_some()));
        self.activate_tab(landing.min(self.window.tabs.len() - 1), true)
    }

    /// Answer the prompt and put it away.
    ///
    /// **The answer is recorded rather than spent** (multiwindow slice D). One
    /// press has to reach every open window and open the windows that are not
    /// open yet, and a `Runtime` is one window by construction — so it goes where
    /// the other two things a window can ask of the process go, and the loop's
    /// door spends it. `App::pending_new_windows`'s shape, for its reason.
    pub(in crate::runtime) fn answer_restore_prompt(
        &mut self,
        answer: restore::RestoreAnswer,
    ) -> Result<()> {
        self.window.restore_prompt.close();
        self.app.pending_restore_answer = Some(answer == restore::RestoreAnswer::Restore);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **The accent ring another window's menu is pointing at** (B9, user
    /// ruling 2026-08-25), or nothing when no menu is pointing here.
    ///
    /// A window with no name of its own cannot be picked out of a list by its
    /// words alone, so the list says which one it means by marking the window
    /// itself — the same answer the drop overlay gives about a rectangle, in the
    /// same accent and drawn by the same halo, one container up.
    ///
    /// **Inside the frame and not around it.** The ring this window can draw is
    /// the one it owns pixels for; a mark outside the client area would be a
    /// mark the compositor never asked for. So it stands on the inner edge,
    /// which is where every other ring in this build stands (a selected tree
    /// row, a focused card).
    pub(in crate::runtime) fn window_ring_layer(&self) -> Vec<marks::OverlayLayer> {
        if self.app.window_ring != Some(self.window_id()) {
            return Vec::new();
        }
        let scale = self.window.renderer.scale_factor() as f32;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        if width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }
        let stroke = (bt_render::DOCK_PREVIEW_BORDER_LOGICAL_PX * scale)
            .round()
            .max(1.0);
        let radius = bt_render::DOCK_PREVIEW_RADIUS_LOGICAL_PX * scale;
        let inner = [stroke, stroke, width - stroke, height - stroke];
        if inner[2] <= inner[0] || inner[3] <= inner[1] {
            return Vec::new();
        }
        let alpha = f32::from(bt_render::DOCK_SHIFT_BORDER_ALPHA) / 255.0;
        let quads = bt_render::rounded_overlay_halo(
            inner,
            (radius - stroke).max(0.0),
            stroke,
            bt_render::chrome_palette().accent,
            alpha,
        );
        if quads.is_empty() {
            return Vec::new();
        }
        vec![marks::OverlayLayer {
            quads,
            ..Default::default()
        }]
    }

    /// Ask the OS for the work area of the display this window is on.
    ///
    /// tiny-window §4.4: a query that fails leaves the last successful answer in
    /// place, because the work area rarely changes between two queries and
    /// reusing the old number is more honest than inventing one. Having never
    /// succeeded is a different state with a different answer — no minimum at
    /// all, rather than a guess that could lock the user's window.
    pub(in crate::runtime) fn refresh_work_area(&mut self) {
        #[cfg(target_os = "linux")]
        {
            if self.window.pending_work_area.is_some() {
                self.window.work_area_refresh_owed = true;
                return;
            }
            let Ok(native) = native_window(&self.window.window) else {
                return;
            };
            let generation = self.app.next_display_generation();
            if let Ok(request) = bt_platform::linux_display::request_display(
                u64::from(self.window.window.id()),
                generation,
                bt_platform::linux_display::LinuxDisplayQuery::WindowWorkArea { window: native },
            ) {
                self.window.pending_work_area = Some(request);
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let Ok(native) = native_window(&self.window.window) else {
                return;
            };
            let Ok(rect) = bt_platform::get_work_area(native) else {
                return;
            };
            let scale = self.window.renderer.scale_factor().max(f64::MIN_POSITIVE);
            let width = ((rect.right - rect.left).max(0) as f64 / scale).round() as i64;
            let height = ((rect.bottom - rect.top).max(0) as f64 / scale).round() as i64;
            self.window.work_area = WorkAreaHint::Known(bt_layout::LogicalSize::px(width, height));
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn apply_linux_work_area_ready(
        &mut self,
        ready: bt_platform::linux_display::LinuxDisplayReady,
    ) -> Result<bool> {
        let matches = self
            .window
            .pending_work_area
            .as_ref()
            .is_some_and(|request| request.ready() == ready);
        if !matches {
            return Ok(false);
        }
        let Some(request) = self.window.pending_work_area.take() else {
            return Ok(false);
        };
        if std::mem::take(&mut self.window.work_area_refresh_owed) {
            self.refresh_work_area();
            return Ok(true);
        }
        if let Ok(bt_platform::linux_display::LinuxDisplayAnswer::WindowWorkArea(Ok(rect))) =
            request.try_take()
        {
            let scale = self.window.renderer.scale_factor().max(f64::MIN_POSITIVE);
            let width = ((rect.right - rect.left).max(0) as f64 / scale).round() as i64;
            let height = ((rect.bottom - rect.top).max(0) as f64 / scale).round() as i64;
            self.window.work_area = WorkAreaHint::Known(bt_layout::LogicalSize::px(width, height));
            self.apply_window_min_inner_size()?;
        }
        Ok(true)
    }

    /// Hand the OS the technical floor — one pane, whatever the tabs contain.
    ///
    /// **User ruling 2026-08-08.** This used to hand over the largest minimum any tab tree needed,
    /// so a single four-column tab decided how narrow the user's window was allowed to be, for
    /// every tab, forever. That aggregate is gone: it was law aimed at the wrong party. The layout
    /// needs it named have not been repealed, they are enforced where the *program* lays out
    /// (`SizePolicy::Lawful`), and the window is left to the hand that owns it.
    ///
    /// What is left is the floor below which this program does not run — one terminal leaf, the
    /// same constant `sane_restored_size` refuses a saved rectangle under. It is the same for every
    /// tab, which is why there is no aggregate any more rather than an aggregate of one value:
    /// activation cannot alternate a constraint that does not vary.
    ///
    /// The constraint goes to the frame rather than to `Window::set_min_inner_size`: winit 0.30
    /// implements that setter by re-requesting the current inner size, and re-requesting runs
    /// `AdjustWindowRectExForDpi`, which adds a native frame margin this self-drawn window does
    /// not wear. Every call grew the window by that margin. `CustomWindowFrame` states the same
    /// minimum through `WM_GETMINMAXINFO` instead, which asks for no resize at all.
    pub(crate) fn apply_window_min_inner_size(&mut self) -> Result<()> {
        let metrics = self.seat_metrics();
        let floor = self.seats.min_inner_size(&metrics, self.window.work_area);
        let minimum = (
            floor.width.floor_px().max(1),
            floor.height.floor_px().max(1),
        );
        if !window_minimum_changed(&mut self.window.window_min_inner_size, minimum) {
            return Ok(());
        }
        self.window
            .custom_window_frame
            .set_min_client_size(Some((minimum.0.max(0) as u32, minimum.1.max(0) as u32)))
            .map_err(|error| anyhow!(error))
            .context("apply the window's minimum client size")
    }

    /// The durable form of everything **this window** would want back after a
    /// restart. Layout *intent* only (L11): no rectangle, no cols/rows, no DPI
    /// of a seat — those are all recomputed by the next `solve`.
    ///
    /// One window's paragraph of the document and not the document (multiwindow
    /// slice D). What is here is what schema v9 moved inside `windows[]`, which
    /// is `docs/DESIGN.md` §2.4's own question asked about durability: the
    /// rectangle, the rail's resting shape, the tabs and which one was on top.
    /// The theme, the cursor and the vault are the *process's* and are written by
    /// [`App::session_document`], once, over every window's answer to this.
    pub(crate) fn window_snapshot(&mut self) -> SessionWindowV1 {
        #[cfg(target_os = "linux")]
        {
            self.queue_linux_window_rect_snapshot();
            self.window_snapshot_with_rect(|| self.window.last_winit_rect)
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.window_snapshot_with_rect(|| {
                let native = native_window(&self.window.window).ok()?;
                bt_platform::get_window_rect(native).ok()
            })
        }
    }

    #[cfg(target_os = "linux")]
    fn queue_linux_window_rect_snapshot(&mut self) {
        if bt_platform::linux_display::active_backend()
            != Some(bt_platform::linux_window::Backend::X11)
            || self.window.leaving.is_some()
        {
            return;
        }
        if self.window.pending_window_rect.is_some() {
            self.window.window_rect_refresh_owed = true;
            return;
        }
        let Ok(native) = native_window(&self.window.window) else {
            return;
        };
        let generation = self.app.next_display_generation();
        if let Ok(request) = bt_platform::linux_display::request_display(
            u64::from(self.window.window.id()),
            generation,
            bt_platform::linux_display::LinuxDisplayQuery::WindowRect { window: native },
        ) {
            self.window.pending_window_rect = Some(request);
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn apply_linux_window_rect_ready(
        &mut self,
        ready: bt_platform::linux_display::LinuxDisplayReady,
    ) -> Result<bool> {
        let matches = self
            .window
            .pending_window_rect
            .as_ref()
            .is_some_and(|request| request.ready() == ready);
        if !matches {
            return Ok(false);
        }
        let Some(request) = self.window.pending_window_rect.take() else {
            return Ok(false);
        };
        if self.window.leaving.is_some() {
            return Ok(true);
        }
        if std::mem::take(&mut self.window.window_rect_refresh_owed) {
            self.queue_linux_window_rect_snapshot();
            return Ok(true);
        }
        if let Ok(bt_platform::linux_display::LinuxDisplayAnswer::WindowRect(Ok(facts))) =
            request.try_take()
        {
            self.window.last_winit_rect = Some(facts.rect);
            self.window.native_client_origin = facts.client_origin;
            self.window.native_window_minimized = facts.minimized;
            if let Some(crate::WindowMaximizeAction::Request(target)) =
                self.window.maximize_intent.observe(facts.maximized)
            {
                self.request_linux_window_maximized(target);
            }
            let snapshot = self.window_snapshot_with_rect(|| Some(facts.rect));
            self.app
                .record_window(self.window.window.id(), snapshot, Instant::now());
        }
        Ok(true)
    }
    fn window_posture(&self) -> WindowPosture {
        crate::choose_window_posture(self.window_minimized_state(), self.window_maximized_state())
    }

    fn window_snapshot_with_rect(
        &self,
        rect: impl FnOnce() -> Option<bt_platform::WindowRect>,
    ) -> SessionWindowV1 {
        // **Asked once for the paragraph**, because three things below read it and
        // one of them is a per-tab decision: which caption run this window wears is
        // not the question here, but which *kind* of window this is decides its
        // `quake` flag, its arranged rectangles, and whether a pinned tab writes
        // down the line it last ran (§7.54e ④).
        let is_quake = self.is_quake_window();
        let persist_quake_record = is_quake || self.window.restored_quake_as_ordinary;
        let previous = self.app.window_picture(self.window.window.id());
        let scale = self.window.renderer.scale_factor().max(f64::MIN_POSITIVE);
        let posture = self.window_posture();
        // The window's *outer* rect, which the self-drawn frame has made the same
        // rectangle as its client area — the one thing `startup_window_rect` can
        // hand back to Win32 without anything in between adjusting it.
        //
        // Measured only while the window is normal, because that is the only
        // posture whose rectangle is the user's; `recorded_window_placement`
        // states what the other two record instead.
        let measured = (posture == WindowPosture::Normal)
            .then(rect)
            .flatten()
            .map(|rect| persisted_window_bounds(rect, scale));
        // What this window last said about itself, which is what the two
        // postures that have no rectangle of their own fall back to. A window
        // that has never said anything falls back to the product's placeholder,
        // which is `recorded_window_placement`'s own contract read with no prior
        // reading to offer it.
        let was = previous
            .map(|window| window.placement.clone())
            .unwrap_or_default();
        let (bounds, maximized) =
            recorded_window_placement(posture, measured, was.bounds, was.maximized);
        let tabs = self
            .window
            .tabs
            .iter()
            .map(|tab| TabV1 {
                // Every terminal leaf is asked about itself, so a tab whose two
                // panes stand in two folders writes two folders — and every
                // files leaf likewise, so a column that was rooted somewhere
                // writes *there* instead of the empty string it used to be
                // flattened to on the way past.
                // **A remembered command line is written for one kind of tab and no
                // other** (§7.54e ④): a pinned tab of the summoned terminal. The
                // pin is the reader saying this tab is a standing thing, and a
                // document that collected the last line every pane in every window
                // ran would be keeping a command history nobody asked it to keep.
                root: tab.seats.to_persisted(
                    &|seat| tab.term_leaf(seat, persist_quake_record && tab.pinned),
                    &|seat| tab.files_state(seat),
                ),
                pinned: tab.pinned,
                // Positional rather than a stable id: the in-order index is a function of the
                // same tree shape the file carries, so it cannot point outside that tree.
                focused_leaf: format!("leaf-{}", focus_leaf_index(&tab.seats)),
                // And what each preview pane was showing, out of a pool that
                // records its files and never their bodies (P151).
                preview: tab.preview_content(),
            })
            .collect();
        let mut window = SessionWindowV1 {
            placement: WindowStateV1 {
                bounds,
                dpi: self.window.renderer.dpi_milli().get(),
                maximized,
                monitor_id: was.monitor_id,
            },
            tab_layout: session_tab_layout(self.window.rail.layout),
            sidebar_mode: session_sidebar_mode(self.window.rail.mode),
            tabs,
            active_tab: self.window.active_tab as u32,
            // **The one fact about this window that is neither where it is nor
            // what is in it** (§7.54). Its placement above is written like every
            // other window's and is deliberately never read back — a summon
            // computes its rectangle from the monitor the pointer is on, every
            // time. See `quake::Quake::placement`.
            quake: persist_quake_record,
            // **And the rectangles a hand made**, which are the one thing about
            // this window that *is* read back — filed under the display they
            // were made on, so that the objection above stays answered. Empty
            // for every other window, because only this one has them.
            quake_placements: if persist_quake_record {
                self.app.quake.placements()
            } else {
                Vec::new()
            },
        };
        // A question that was never answered is not a "no". Tabs still waiting on
        // the restore prompt go back to the file exactly as they came out of it,
        // so closing the window mid-question asks again next time rather than
        // deciding on the user's behalf (§7.1.4: "未答复计划并回 lastSession,
        // 不得丢失"). They are appended unpinned, which is what they were.
        window
            .tabs
            .extend(self.window.pending_restore.iter().map(|tab| TabV1 {
                pinned: false,
                ..tab.clone()
            }));
        window
    }

    /// Record a meaningful change and start the debounce window (§5.1).
    ///
    /// **From every window, into its own slot** (multiwindow slice D). Slice C
    /// had to turn all but one window away here, because the file held one
    /// window; schema v9 holds them all, so what this does instead is put *this*
    /// window's paragraph where it belongs and hand the assembled document to the
    /// one store. Two windows that both change something in the same frame
    /// therefore produce one write and not two — see [`App::record_window`].
    pub(crate) fn mark_session_dirty(&mut self, now: Instant) {
        let snapshot = self.window_snapshot();
        let id = self.window.window.id();
        self.app.record_window(id, snapshot, now);
    }

    /// **The window's ground, put in force** (§7.1.6c-4b) — the one place the
    /// picture, the fit and the two percentages reach the renderer.
    ///
    /// One function and not four, because they are one value: `set_window_ground`
    /// takes the whole ground and answers `Unchanged` when nothing moved, so a
    /// settings write that touched something else costs no revision and no
    /// repaint. Every caller here (a row, a chooser, startup) goes through this,
    /// which is what keeps "what is stored" and "what is drawn" one reading
    /// rather than four that have to agree.
    pub(crate) fn apply_window_ground(&mut self) -> Result<bool> {
        let stored = self.app.settings_store.loaded();
        let ground = bt_render::WindowGround {
            image: self.window.background_picture.clone(),
            fit: match stored.background_fit {
                bt_persist::BackgroundFitV1::Stretch => bt_render::BackgroundFit::Stretch,
                bt_persist::BackgroundFitV1::Fill => bt_render::BackgroundFit::Fill,
                bt_persist::BackgroundFitV1::Tile => bt_render::BackgroundFit::Tile,
            },
            image_opacity: f32::from(stored.background_image_opacity) / 100.0,
            // A machine whose surface is opaque draws an opaque ground whatever
            // the file says. Not a clamp on the stored value — the file keeps
            // what its owner wrote, and moving the profile to a machine that can
            // honour it restores the window they set up.
            alpha: if self.window.translucency_available {
                f32::from(stored.background_opacity) / 100.0
            } else {
                1.0
            },
        };
        if bt_render::set_window_ground(ground) == bt_render::ThemeChange::Unchanged {
            return Ok(false);
        }
        // The clear colour moved, so the class background brush behind it has to
        // as well — it is what shows in the band a resize opens up, and a window
        // whose ground is half see-through must not flash an opaque rectangle
        // there. `adopt_new_palette` is the same path a scheme change takes, and
        // for the same reason: the ground rides `theme_revision`.
        self.adopt_new_palette()?;
        Ok(true)
    }

    /// **Which canvas this window tells DWM it is wearing** (§7.1.6c-4f
    /// amendment) — and therefore how dark DWM tints the acrylic plate behind
    /// the ground, and what colour it draws the window's one-pixel border.
    ///
    /// Read off `background_rgb()` and not off the settings file, for
    /// `scheme_in_force`'s reason: the answer has to be about the colours the
    /// glass is actually showing. A `BT_BG` override, or a "dark scheme" whose
    /// file happens to name a pale background, both get the plate their own
    /// luma asks for rather than the one their row is called.
    ///
    /// Best-effort and silent about the ordinary case: an old Windows refuses
    /// the attribute and keeps the border it had.
    pub(in crate::runtime) fn apply_window_dark_mode(&mut self) -> Result<()> {
        let Some(dark) = dwm_dark_mode_owed(self.window.dwm_dark_mode, bt_render::background_rgb())
        else {
            return Ok(());
        };
        let native = native_window(&self.window.window)?;
        self.window.dwm_dark_mode = Some(dark);
        if let Err(error) = bt_platform::set_window_dark_mode(native, dark) {
            eprintln!("recoverable dark-mode failure: {error}");
        }
        Ok(())
    }

    /// **Whether something is standing in front of this window that a drop must
    /// not go under** (review 2026-09-17 P1-b).
    ///
    /// The line this draws is the one [`Self::focus_the_pane_a_path_landed_in`]
    /// is the other side of. A files column, a preview and the search capsule
    /// borrow the keyboard and can simply be asked for it back, so a drop takes
    /// it. A quit card, a settings page, the dirty gate, the first-run card, the
    /// PSReadLine invitation, any open menu or popup, the command palette and
    /// the tab-name box are **answered**: they own the keyboard because the
    /// window is waiting for a reply, and the reply is `Enter`.
    ///
    /// So a file let go of over one of them is refused outright rather than
    /// written into whatever terminal happens to be behind it. Three things
    /// would otherwise all be wrong at once: a path typed into a pane the reader
    /// cannot see, a window brought to the front over the card it is asking
    /// about, and — worst — the next `Enter` answering the card instead of
    /// running the command they just built.
    ///
    /// Read off [`Self::keyboard_owner`] rather than from a list of surfaces of
    /// its own, so that a modal added to this window is covered by this the day
    /// it takes the keyboard.
    pub(in crate::runtime) fn a_modal_holds_the_window(&self) -> bool {
        self.keyboard_owner().is_modal()
    }

    /// **Bring this window to the front, because a drop is the reader pointing
    /// at it** (owner's ruling 2026-09-17).
    ///
    /// The two steps [`FolioApp::raise_for_a_launch`] makes for a second start,
    /// from the window's own side and for the same reason: un-minimise first,
    /// because a foreground call on an iconified window brings it forward on
    /// some configurations without restoring it, and then the one platform door
    /// that actually takes the keyboard.
    ///
    /// **This is not focus stealing, and the gesture is why.** A file let go of
    /// over this window is the reader saying "this one, here" with their hand;
    /// answering it by typing a path into a window that stays behind the one
    /// they dragged from is the drop half-honoured. It happens on no other road:
    /// a clipboard paste is made *in* this window and an internal drag never
    /// left it.
    ///
    /// `bt_platform::hotkey::give_foreground_to` and not winit's
    /// `focus_window()`, because only one of the two is enough on macOS:
    /// `makeKeyAndOrderFront:` raises a window inside its own application, and
    /// the call there pairs it with `-[NSApplication activate]` so the
    /// application itself becomes the frontmost one. On Windows it is the
    /// foreground-lock dance with the answer read back. **Failure is silent to
    /// the reader and one line in the log**, which is that door's own rule:
    /// there is nothing a person can do about a foreground lock, and the path is
    /// on the command line either way.
    pub(in crate::runtime) fn bring_this_window_forward(&mut self) {
        if let Err(error) = crate::bring_owned_window_forward(&self.window.window) {
            eprintln!("BT_DROP {error}");
        }
    }

    /// **This window, as a worker addresses it.**
    ///
    /// The outer half of every address on the four decoration lanes. See
    /// [`ShellAddress`] for why a `TabId` is not enough on its own: the counters
    /// that mint the inner halves start again in every window, and the channels
    /// the answers come home on are the application's.
    pub(crate) fn window_id(&self) -> WindowId {
        self.window.window.id()
    }

    /// Put the question, and report what putting it came to.
    ///
    /// [`restore::GateRaise::Raised`] means the gate is now up and the caller
    /// must stop — which is the whole protocol: a gate is not a callback, it is
    /// a *pause*, and the verb that was interrupted is re-run from the top once
    /// it is answered ([`Self::answer_dirty_gate`] takes the request off the gate
    /// *before* it re-runs anything, so the re-run finds the gate free).
    ///
    /// **A gate that is already up answers [`restore::GateRaise::Busy`], and busy
    /// never lets the caller go on** (ticket 58). It used to answer `false`, the
    /// word for "nothing to ask", so an OS close requested while a tab's close
    /// was still being asked about shut the window and dropped the buffer the
    /// question was about. Only [`restore::GateRaise::NothingToAsk`] authorises
    /// the verb ([`restore::GateRaise::proceeds`]).
    pub(crate) fn raise_dirty_gate(
        &mut self,
        request: restore::GateRequest,
    ) -> Result<restore::GateRaise> {
        let window = &mut *self.window;
        let raised = crate::raise_dirty_gate_over(
            &mut window.dirty_gate,
            &window.tabs,
            window.active_tab,
            request,
        );
        if raised == restore::GateRaise::Raised && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(raised)
    }

    /// **Put a question whose verb only the answer performs** — the git
    /// requests, where the gate stands in front of the write rather than behind
    /// an interrupted verb, so the confirmed answer in [`Self::answer_dirty_gate`]
    /// is where the repository is asked.
    ///
    /// Nothing is done here whatever the gate says: raised, the reader is asked;
    /// busy, the reader is already being asked about something else and this
    /// press is dropped (ticket 58). Every git request names what it is about
    /// ([`crate::dirty_gate_names`]), so it is never nothing to ask.
    pub(in crate::runtime) fn ask_before_the_verb(
        &mut self,
        request: restore::GateRequest,
    ) -> Result<()> {
        let _raised_or_busy = self.raise_dirty_gate(request)?;
        Ok(())
    }

    /// Spend the answer.
    ///
    /// **Discard empties the pool first and then re-runs the verb**, in that
    /// order and never the other: the gate raises itself off the pool, so a
    /// re-run against a pool still holding dirty buffers would put the same
    /// question again forever.
    pub(in crate::runtime) fn answer_dirty_gate(
        &mut self,
        answer: restore::GateAnswer,
    ) -> Result<()> {
        let Some(request) = self.window.dirty_gate.take() else {
            return Ok(());
        };
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        match (request, answer) {
            // **The two exits answer all three buttons in one place**
            // ([`Self::answer_exit`]): a window's own shut, and the run's end
            // asked in the summoned terminal, whose confirmed answer re-runs the
            // close of the ordinary window that ends it.
            (restore::GateRequest::Shut, answer) => self.answer_exit(self.window_id(), answer),
            (restore::GateRequest::ShutWithTheRun(closing), answer) => {
                self.answer_exit(closing, answer)
            }
            // "取消不关" — nothing happens, and nothing is lost. The one answer a
            // gate must be able to give.
            (_, restore::GateAnswer::Cancel) => Ok(()),
            // Only an exit offers `Save all` (`GateRequest::offers_save`), so from
            // here the answer goes through with the request: the button that
            // would send any of these down a save is not drawn.
            (restore::GateRequest::ClosePane(seat), _) => {
                // The pool is the tab's, so emptying it leaves *every* surface
                // naming a buffer that is gone — the view each of them was on has
                // to be filed and let go, not only the pane being closed.
                self.preview_pool.clear();
                for surface in self.preview_surfaces() {
                    self.clear_preview_view(surface);
                }
                self.close_pane(seat)
            }
            (restore::GateRequest::CloseTab(index), _) => {
                if let Some(tab) = self.window.tabs.get_mut(index) {
                    tab.preview_pool.clear();
                }
                self.close_tab(index)
            }
            // The one request whose confirmed verb is not a re-run of something
            // that was interrupted: nothing was in flight, because the gate is in
            // front of the write rather than behind it. So this is where the
            // question is actually asked of the repository (R13's pessimism
            // starts one line later, when the row dims).
            (
                restore::GateRequest::GitDiscard {
                    origin,
                    path,
                    untracked,
                },
                _,
            ) => {
                let verb = if untracked {
                    git::GitWriteVerb::DiscardUntracked
                } else {
                    git::GitWriteVerb::Discard
                };
                self.issue_git_write(&origin, verb, vec![path])
            }
            // **The two ref deletions** (v2 ④), keyed by root rather than by
            // seat: the surface that asked may have gone while the gate stood,
            // and either surface on this repository can carry the write — every
            // cache on it re-reads when the receipt lands
            // ([`git::GitWriteVerb::moves_refs`]).
            (restore::GateRequest::GitDeleteBranch { root, name }, _) => {
                let Some(origin) = self.git_origin_for_root(&root) else {
                    return Ok(());
                };
                self.issue_git_write(
                    &origin,
                    git::GitWriteVerb::DeleteBranch { name },
                    Vec::new(),
                )
            }
            (restore::GateRequest::GitDeleteTag { root, name }, _) => {
                let Some(origin) = self.git_origin_for_root(&root) else {
                    return Ok(());
                };
                self.issue_git_write(&origin, git::GitWriteVerb::DeleteTag { name }, Vec::new())
            }
            // **The detaching checkout**, on the two deletions' own shape: the
            // gate stands in front of the verb rather than behind it, so this is
            // where the repository is actually asked.
            (
                restore::GateRequest::GitCheckout {
                    root, target, kind, ..
                },
                _,
            ) => {
                let Some(origin) = self.git_origin_for_root(&root) else {
                    return Ok(());
                };
                self.checkout_at(&origin, target, kind)
            }
            // The gate stands *in front of* this one too (`GitDiscard`'s shape),
            // so the confirmed answer is the deletion itself rather than a re-run
            // of something that was interrupted.
            (restore::GateRequest::ClearScrollback(seat), _) => self.clear_pane_scrollback(seat),
        }
    }

    /// **Spend an answer to one of the two exits**, whose confirmed verb is the
    /// close of `closes` — this window for its own shut, the run's last ordinary
    /// window for the run's end asked in the summoned terminal
    /// (T-SUMMON-DIRTY-PREVIEW).
    ///
    /// **`Save all` writes first and closes only if all of it landed** (B1,
    /// user ruling 2026-08-25), which is [`quit::Quit::saved`]'s own rule one
    /// surface down and for its reason: a shut that closed the window after a
    /// half-finished save would take the half that is still only in memory
    /// with it. The failures are already named on their own pane by
    /// `quit_save`; the window stays, so the reader can see them there.
    ///
    /// **`Discard` drops only the dirty buffers**: every tab is about to be
    /// written to `session.json`, and its pool goes with it as the list of files
    /// the switcher will list next launch (`TabState::preview_content`) —
    /// measured on the real machine, where emptying it wrote `"pool": []` and a
    /// three-file history came back empty. See [`crate::answer_an_exit_over`],
    /// which is the whole of this but the write.
    ///
    /// The close is the one verb this does not own: it is the event loop's, and
    /// it is re-requested ([`crate::WindowRuntime::window_close_requested`]) rather than
    /// performed here so that everything else a close does still happens in the
    /// order it always did — and, for the run's end, so that `Cancel` leaves the
    /// ordinary window open and the summoned terminal never stands alone.
    fn answer_exit(&mut self, closes: WindowId, answer: restore::GateAnswer) -> Result<()> {
        let saved_all = answer == restore::GateAnswer::Save && self.quit_save()?.is_complete();
        self.window.window_close_requested =
            crate::answer_an_exit_over(&mut self.window.tabs, closes, answer, saved_all);
        Ok(())
    }

    /// **What this window would lose if the process left now**, by name.
    ///
    /// One window's contribution to the summary card's one list. Two classes,
    /// and the boundary between them is *what outlives the quit*:
    ///
    /// * **Dirty preview buffers.** A pool carries paths and never bodies
    ///   (P151), so an edited buffer that is not written back is gone the moment
    ///   the process is — the same fact the shut gate is built on, asked about
    ///   every window instead of this one.
    /// * **An open editor whose draft is an actual edit** — see
    ///   [`Self::uncommitted_edit_name`].
    ///
    /// Every tab's pool and not the active tab's, on the shut gate's own
    /// reasoning: a dirty buffer on a tab nobody is looking at is still a dirty
    /// buffer.
    pub(crate) fn quit_dirty_names(&self) -> Vec<String> {
        self.window
            .tabs
            .iter()
            .flat_map(|tab| {
                let where_ = tab.display_title();
                tab.preview_pool
                    .dirty_names(None)
                    .map(|name| unsaved_line(name, &where_))
                    .collect::<Vec<_>>()
            })
            .chain(self.uncommitted_edit_name())
            .collect()
    }

    /// **Write this window's half of the save branch back** (slice E2 phase ①,
    /// v3).
    ///
    /// Item by item through the pool's own [`preview::PreviewPool::save_dirty`],
    /// which is `Ctrl+S`'s conflict check and `Ctrl+S`'s atomic write; then the
    /// open editor, committed exactly as Enter commits it. What went through and
    /// what did not are both reported, because a save that half worked leaves a
    /// state no single sentence describes.
    ///
    /// A conflict is a failure *here* even though it is not one at the pane
    /// (`SaveOutcome`'s own note: "the disk moved and the window is declining to
    /// guess"). The difference is what happens next: at the pane the buffer
    /// survives and the reader can look; at a quit the process was about to
    /// leave, and leaving would take the buffer with it.
    pub(crate) fn quit_save(&mut self) -> Result<quit::SaveReport> {
        let mut report = quit::SaveReport::default();
        for tab in &mut self.window.tabs {
            for (name, outcome) in tab.preview_pool.save_dirty() {
                match outcome {
                    preview::SaveOutcome::Saved => report.saved.push(name),
                    preview::SaveOutcome::Conflict => report
                        .failed
                        .push((name, preview::preview_conflict_notice().to_owned())),
                    preview::SaveOutcome::Failed(error) => {
                        eprintln!("recoverable preview save failure: {error}");
                        report.failed.push((name, error));
                    }
                }
            }
        }
        if let Some(name) = self.uncommitted_edit_name() {
            self.finish_rename(RenameExit::Blur)?;
            report.saved.push(name);
        }
        // **The failures are named where they happened.** One card per window
        // holding one, rather than a single card in whichever window the chord
        // was pressed in: a notice belongs where the attention already is, and
        // the buffers this sentence is about are in *this* window.
        if !report.failed.is_empty() {
            let names = report.failed_names().join(", ");
            self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::Window,
                None,
                i18n::quit_not_saved(&names),
            )?;
        }
        // The panes that were showing those buffers are looking at what is now on
        // the disk, and the ones that failed are still dirty and still say so.
        self.repaint_preview()?;
        Ok(report)
    }

    /// **Keep what this window would lose, on a stop that cannot ask** (D-4,
    /// 0.4.8 G7): every dirty preview buffer of every tab, through the quit's
    /// judged write and, where that is refused, into `recovery`
    /// ([`crate::keep_unsaved_edits_over`]), each said in `diagnostics.log` with
    /// where its edit is. No card and no repaint: the window is about to be
    /// closed by a process that is stopping.
    pub(crate) fn keep_unsaved_edits(&mut self, recovery: &Path) {
        for kept in
            crate::keep_unsaved_edits_over(&mut self.window.tabs, recovery, SystemTime::now())
        {
            crate::diagnostics::note(&kept.line());
        }
    }

    /// **Take this window off the screen and let go of everything it holds**
    /// (slice E2 phase ④).
    ///
    /// [`Self::close_window`]'s second half with its first half deliberately
    /// absent. The picture has been taken and the document is on the disk; a
    /// window that photographed itself again here would be re-recording after the
    /// teardown of the windows before it had already begun — the interleaving
    /// §2.9 exists to forbid — and would hand the store a second document after
    /// the one this quit already judged.
    ///
    /// Hidden rather than dropped, because the loop is still running: the wait
    /// for the browsers to go needs windows to keep turning
    /// ([`Self::advance_retirement`]), and a reader must not be looking at three
    /// dead terminals while it does.
    pub(crate) fn retire_window(&mut self) -> Result<()> {
        self.let_go_of_this_window()
    }

    /// **The other windows, as the submenu names them** (B9, user ruling
    /// 2026-08-25) — in the order they were opened, which is the order the
    /// ordinals count in.
    ///
    /// Read off [`App::windows_open`], which is the directory `FolioApp`
    /// publishes at each turn's head and as each window opens: a `Runtime` is
    /// one window by construction and can
    /// see no other, so the list has to be handed down rather than walked here.
    ///
    /// **This window is not in it.** A pane already in this window has nothing
    /// to move, and a row that did nothing would be a row.
    pub(in crate::runtime) fn other_window_rows(&self) -> Vec<String> {
        let here = self.window.window.id();
        self.app
            .windows_open
            .iter()
            .filter(|open| open.id != here)
            .map(|open| i18n::window_row(open.ordinal, open.tabs))
            .collect()
    }

    /// The same list as ids, in the same order — what a press on row `n` is
    /// about.
    pub(in crate::runtime) fn other_window_ids(&self) -> Vec<WindowId> {
        let here = self.window.window.id();
        self.app
            .windows_open
            .iter()
            .filter(|open| open.id != here)
            .map(|open| open.id)
            .collect()
    }

    /// **Ring the window this menu row is pointing at** (B9, user ruling
    /// 2026-08-25), or take the ring off.
    ///
    /// Written on the application because the window that draws it is not the
    /// window the pointer is in — the whole point of the mark. `FolioApp` spends
    /// it in the same turn (`settle_window_ring`), which is
    /// [`App::pending_new_windows`]'s standing shape and its standing reason.
    pub(in crate::runtime) fn aim_at_window(&mut self, window: Option<WindowId>) {
        self.app.window_ring = window;
    }

    /// One floating window, drawn — dispatched on **who is inside it**.
    ///
    /// The chassis is shared and the two tenants differ only in what fills the
    /// three strips, which is P49's difference table made structural: one branch
    /// per tenant, both handing the same [`float::build`] a [`float::FloatChrome`]
    /// and a body.
    pub(in crate::runtime) fn float_window(
        &mut self,
        id: float::FloatId,
        now: Instant,
    ) -> Option<marks::OverlayLayer> {
        if self
            .window
            .float
            .drawn()
            .find(|win| win.epoch == id)?
            .preview()
            .is_some()
        {
            return self.preview_float_layer(id, now);
        }
        self.files_float_layer(id, now)
    }

    /// **The hand opened somewhere this window does not own** (multiwindow slice
    /// F2).
    ///
    /// Answers whether the release has been taken over. Everything it can do
    /// here it does — J120's settle, so the tab list this payload was travelling
    /// through goes back to the order it was in — and everything it cannot, it
    /// writes down: moving a tab into another window, or opening a window to hold
    /// it, are [`FolioApp`]'s and are spent at the loop's door in this same turn.
    pub(in crate::runtime) fn hand_over_across_windows(&mut self, drag: &Drag) -> Result<bool> {
        let Some(broker) = self.app.drag_broker.as_ref() else {
            return Ok(false);
        };
        // Only the window that is holding the payload may spend the broker's
        // answer. Any other window reaching this line is a button-up that did not
        // belong to this gesture.
        if broker.source != self.window_id() {
            return Ok(false);
        }
        #[cfg(target_os = "linux")]
        let guard_request_pending = self
            .app
            .pending_drag_guard_screen
            .as_ref()
            .is_some_and(|pending| pending.broker_generation == broker.guard_generation);
        #[cfg(target_os = "linux")]
        if !broker.guarded_release_is_current(guard_request_pending, broker.pointer) {
            self.settle_home(drag);
            return Ok(true);
        }
        let verdict = broker_verdict(&broker.cargo, &broker.aim);
        // The road's second station ([`Runtime::foreign_strip_landing`] is the
        // first): **what the release decided, and off which aim**. Formatted
        // ahead of the closure because a `Debug` of the aim is real work — the
        // gate's own rule for the handful of stations that must compute a field
        // rather than merely format one ([`mouse_trace::is_on`]).
        let traced_aim = mouse_trace::is_on().then(|| format!("{:?}", broker.aim));
        self.mouse_trace(|| {
            format!(
                "hand_over_across_windows verdict={verdict:?} aim={}",
                traced_aim.unwrap_or_default()
            )
        });
        if verdict == BrokerRelease::Local {
            return Ok(false);
        }
        let cargo = broker.cargo.clone();
        let grip = broker.grip;
        let pointer = broker.pointer;
        let from = self.window_id();
        // The strip goes home first, on every one of these paths. A reorder made
        // on the way to a drop is part of that drop's gesture (J120), and a
        // payload that has left the window entirely must not leave half of one
        // behind in the tab list it walked through.
        self.settle_home(drag);
        let into = match verdict {
            // Over one of our windows, on nothing it offers: J120's clean
            // nothing, and the settle above is the whole of it.
            BrokerRelease::Nothing | BrokerRelease::Local => return Ok(true),
            BrokerRelease::Into { window, landing } => HandoverInto::Window { window, landing },
            BrokerRelease::NewWindow => HandoverInto::NewWindow {
                pointer: (pointer.0.round() as i32, pointer.1.round() as i32),
                grip,
            },
        };
        self.app.pending_handover = Some(DragHandover { cargo, from, into });
        Ok(true)
    }

    /// **Put the platform's own window buttons on the band this window wears**
    /// (T-MAC-LIGHTS x T-MAC-PILL, the owner's rulings of 2026-09-12 read
    /// together).
    ///
    /// The two rulings meet on one number. A window whose tab strip stands in
    /// its bar wears Folio's 40 and the three lights are centred on it; every
    /// layout that puts the tab list down the side wears a header of the
    /// platform's own height instead, and centring the lights on *that* is
    /// exactly where the platform had them. So there is no second rule for the
    /// vertical layouts — there is one rule, and [`seats::window_band_px`] is
    /// the number it is given.
    ///
    /// **A door and not an argument to `install`, because the band changes while
    /// the window is open.** `Tab layout`, `Sidebar` and focus mode all move a
    /// window between the two bars without relaunching it. So this is said three
    /// times and in one voice: once in [`Self::dress_new_window`], before the
    /// window is ever shown, and again after each of the two writes of the
    /// posture ([`Self::set_rail_state`], [`Self::set_focus_mode`]). What a host
    /// does about it is `bt-platform`'s to decide; one that draws nothing in
    /// this bar answers `Ok(())`.
    ///
    /// **Read off the window and not off a caller's arguments**, and after the
    /// write rather than before it: the band comes from [`Self::rail_posture`],
    /// so the buttons follow the posture the stage is about to be solved with
    /// and not the one it had.
    ///
    /// **Logical pixels**, which is the unit the frame speaks
    /// ([`bt_platform::CustomFrameGeometry`]) and the unit AppKit places a
    /// button in; `window_band_px` answers in the physical pixels the stage is
    /// solved in, so the scale it was solved at is divided back out here.
    pub(crate) fn follow_the_window_band(&self) -> Result<()> {
        let scale = self.window.renderer.scale_factor() as f32;
        let band = seats::window_band_px(scale, self.platform_chrome());
        self.window
            .custom_window_frame
            .set_window_band(band / scale)
            .map_err(|reason| anyhow!(reason))
            .context("place the platform's window buttons on this window's band")
    }

    /// Whether this window is iconic — Win32's own answer, and the same one
    /// [`Runtime::window_snapshot`] asks before it believes a rectangle.
    pub(in crate::runtime) fn window_is_iconic(&self) -> bool {
        let iconic = self.window_posture() == WindowPosture::Minimized;
        self.window.diagnostic_minimized.set(iconic);
        iconic
    }

    /// A scale change hands the layout a *logical* rectangle no hand chose.
    ///
    /// The physical window on screen is still the user's and still the size they left it; the
    /// rectangle the solver works in is not, because it was computed from a scale factor that
    /// arrived from the system. So the program takes the layout back (user ruling 2026-08-08, DPI
    /// adjudicated as a program event) and the seats fold rather than turn into slivers. One drag
    /// of the frame returns it, which is the whole of what "advice" means here.
    ///
    /// Claimed *before* the resize below, and with the size that resize will carry, so the
    /// `Resized` Windows sends alongside every `WM_DPICHANGED` is recognised as this program's own
    /// rather than read as a hand on the frame.
    /// **The window moved, so every page in it is told where its own menus
    /// go** (§7.7 ⑩, user report 2026-08-25).
    ///
    /// A composition-hosted page renders into a visual and receives no window
    /// messages, so `ICoreWebView2Controller::NotifyParentWindowPositionChanged`
    /// is the only notice the engine gets that the screen rectangle it hangs its
    /// own windows off has moved. `Bounds` says where the seat is *inside* the
    /// window ([`bt_platform::WebHost::set_bounds`]); this says where the window
    /// is. Both are needed and neither implies the other.
    ///
    /// A refusal is said out loud and dropped: a page whose engine would not
    /// take the notice is a page whose context menu opens in the wrong place,
    /// which is not a reason to fail a window move.
    pub(crate) fn window_moved(
        &mut self,
        position: winit::dpi::PhysicalPosition<i32>,
    ) -> Result<()> {
        self.note_winit_position(position);
        self.remember_summoned_arrangement();
        // **The window may be on another panel now** (owner's report
        // 2026-09-18), and the two displays a window is dragged between are
        // routinely not the same rate. Unconditional for
        // [`Self::reoffer_ime_cursor_area`]'s reason, one line down: "the window
        // is on a different display now" is not a question this program can
        // answer more cheaply than the platform can re-derive the answer.
        self.follow_the_display();
        // The input method's copy of the caret rectangle is in screen
        // coordinates and this is the event that invalidated it
        // (`reoffer_ime_cursor_area`). Unconditional, because "the window is on
        // a different display now" is not a question this program can answer
        // more cheaply than the platform can re-derive the rectangle.
        self.reoffer_ime_cursor_area();
        for web in self.window.web.values() {
            if let Err(error) = web.parent_window_moved() {
                eprintln!("BT_WEB {error}");
            }
        }
        Ok(())
    }

    fn note_winit_position(&mut self, position: winit::dpi::PhysicalPosition<i32>) {
        #[cfg(target_os = "linux")]
        {
            self.window.native_client_origin = None;
            let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
            if width > 0 && height > 0 {
                self.window.last_winit_rect = Some(bt_platform::WindowRect {
                    left: position.x,
                    top: position.y,
                    right: position.x.saturating_add(width as i32),
                    bottom: position.y.saturating_add(height as i32),
                });
            }
            self.queue_linux_window_rect_snapshot();
            if self.window.pending_work_area.is_some() {
                self.window.work_area_refresh_owed = true;
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = position;
    }

    pub(in crate::runtime) fn note_winit_size(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        #[cfg(target_os = "linux")]
        {
            self.window.native_client_origin = None;
            self.window.last_winit_size = Some(size);
            if size.width > 0
                && size.height > 0
                && let Some(rect) = self.window.last_winit_rect.as_mut()
            {
                rect.right = rect.left.saturating_add(size.width as i32);
                rect.bottom = rect.top.saturating_add(size.height as i32);
            }
            self.queue_linux_window_rect_snapshot();
        }
        #[cfg(not(target_os = "linux"))]
        let _ = size;
    }
    /// **Whether a modal card or the settings sheet covers the whole window** — the one reading of
    /// "the window is asking and nothing under it answers". A hosted page is hidden under it, a
    /// wheel notch under it is nobody's once the first-run card and the settings sheet have had
    /// the notches that scroll them, and nothing under it lights on a hover (0.4.5 ticket 57 made
    /// the last two ask this, and put the restore card on it by its one reading).
    pub(crate) fn a_modal_covers_the_window(&self) -> bool {
        self.app.quit.as_ref().is_some_and(quit::Quit::is_asking)
            || self.window.dirty_gate.is_open()
            || self.window.first_run.is_open()
            || self.window.psreadline_invite.is_open()
            || self.paste_card_seat().is_some()
            || self.update_card_is_up()
            || self.window.settings.is_open()
            || self.restore_card_is_up()
    }

    /// **Close this window** (multiwindow slice C).
    ///
    /// Everything here is about one window and would be owed a second time by a
    /// second one: the rename it was in the middle of, the caret it lent the
    /// IME, and the shells it is the only reader of. The process's own half —
    /// dropping the run sentinel — is [`App::finish`], and it happens once, when
    /// the last window has gone.
    ///
    /// The session snapshot is taken here rather than in `App::finish` because
    /// it is a picture of *this window's* tabs, and by the time the last window
    /// has closed there is nothing left to photograph.
    ///
    /// **`ending` is the whole of the close semantics** (multiwindow slice D,
    /// ruling ②). A window closed while others remain is a window the *user*
    /// closed: it leaves the document and its seed goes into the vault, so
    /// "重开丢的窗从 Recent 一步找回" is one press and the file keeps describing
    /// what is actually open. Closing the last one is the *process* ending, which
    /// is this product's own long-standing sentence (§2.5), and there the window
    /// stays in the file — that is what makes the next launch open what you left.
    ///
    /// Nothing is asked either way: closing a window raises no prompt. The one
    /// question a shut can ask is the dirty gate, and it has already been asked,
    /// at the door, before this is reached.
    pub(crate) fn close_window(&mut self, ending: bool) -> Result<()> {
        // §7.1.4: "未提交的重命名在序列化前提交（blur 语义,输入到一半关窗不丢
        // 新名字）". Before the snapshot below, not after — the name has to be on
        // the tab by the time the tab is written down.
        self.finish_rename(RenameExit::Blur)?;
        let now = Instant::now();
        if ending {
            self.mark_session_dirty(now);
        } else {
            self.vault_this_window(SystemTime::now());
            let id = self.window.window.id();
            self.app.forget_window(id, now);
        }
        self.let_go_of_this_window()
    }

    /// **Empty a window that was opened to receive a tab which never arrived**
    /// (multiwindow slice F1c).
    ///
    /// The move was refused, so this window holds nothing but the stand-in it
    /// opened with — a window the reader never asked for. Emptying it before the
    /// shut is what keeps it out of Recent: [`Self::vault_this_window`] records
    /// nothing for a window with no tabs, which is the same rule the emptied
    /// *source* of a move leaves by, rather than a second one written for this
    /// door.
    ///
    /// **Every shell is told**, and none of them is waited for
    /// (T-PANE-CLOSE-OFF-THREAD): each goes to its own teardown thread, which is
    /// where a child that will not die is now said out loud. The tabs are taken
    /// off the window first, as they always were, so nothing here is reaching
    /// into a window that is still holding them.
    pub(crate) fn abandon_a_window_nothing_arrived_in(&mut self) -> Result<()> {
        for mut tab in std::mem::take(&mut self.window.tabs) {
            tab.retire_all_shells();
        }
        self.window.active_tab = 0;
        self.window.placeholder_tab = None;
        Ok(())
    }

    /// **Everything this window is holding on to, let go** — the shut's second
    /// half, said once.
    ///
    /// Split out by multiwindow slice E2 so that a quit can spend it without the
    /// first half: [`Self::close_window`] decides where the window's *record*
    /// goes and then calls this, and [`Self::retire_window`] calls this alone,
    /// because a quit has already recorded every window at once.
    ///
    /// **Every child is told, whatever the one before it answered**, and the
    /// first refusal is what comes back. A loop that stopped at the first
    /// failure would leave the shells after it running behind a window nobody
    /// can see — the one outcome worse than an error, and the reason `fail`'s
    /// own comment says so about windows.
    fn let_go_of_this_window(&mut self) -> Result<()> {
        // **Off the screen first, and on every road that ends a window**
        // (§7.34, user report 2026-08-27: 关掉旧 folio 再开新的,输入法就坏了).
        //
        // Measured on the machine: a window that had hosted a page is **never
        // destroyed** — the process leaves in 200ms and the `HWND` is still a
        // window a minute later, with the browser tree it started still running.
        // Five seconds is Windows' ghosting threshold, so `dwm.exe` puts a
        // `Ghost` window over the corpse, and that ghost takes the foreground
        // and the keyboard focus from the window the reader has just launched.
        // A ghost has **no input context** (`ImmGetDefaultIMEWnd` = 0): Chinese
        // cannot be composed at all and every key rings `MessageBeep`. That is
        // the whole of the report, and the two symptoms are one window.
        //
        // The quit's phase ④ already knew this — [`Self::retire_window`] hid the
        // window before letting go, and said why: 「a reader must not be looking
        // at three dead terminals」. What it did not know is that the sentence is
        // not about the reader's patience but about the window's *lifetime*: an
        // ordinary shut relied on the drop chain to destroy the `HWND`, and the
        // drop chain does not always get to. Hiding does not depend on anything
        // getting round to it, on the browser going, or on why the handle
        // survives: a window that is not on the screen cannot be ghosted and
        // cannot hold the foreground. So the hide moves down here, where every
        // road that ends a window already meets — the ordinary close, the quit's
        // retirement, `exiting`, and the failure stop.
        // An owner-thread door (`doors::SetVisible`, whose station the meter enters). A refusal
        // is a hide that had no effect: the letting go carries on.
        let _ = admitted::<doors::SetVisible, _>(|token| {
            owner_door::set_visible(token, &self.window.window, false);
        });
        hang_watch::during(hang_watch::Station::ImeCaretDestroy, || {
            self.destroy_ime_caret("window_teardown")
        });
        // **The page's controller is closed here, beside the children.** A
        // controller merely dropped leaves a browser process nobody points at
        // for as long as the environment lives — which for the last window is
        // no time at all, and for one window among several is the rest of the
        // session. The wait for that browser to go is the state machine's; on
        // the ordinary shut this window is not around for it, and on a quit the
        // loop is deliberately still turning so that it can be.
        for web in self.window.web.values_mut() {
            let _ = web.close(&self.window.compositor);
        }
        // **Every child is told and none of them is waited for**
        // (T-PANE-CLOSE-OFF-THREAD). Each session comes out of its leaf, so
        // nothing left in this window can reach one that is being taken apart,
        // and each goes to its own teardown thread — which is where a child that
        // will not go is now said out loud, rather than as an error carried back
        // to a window that has already gone. The quit does not walk out in front
        // of them: its `Retire` step waits on
        // [`bt_pty::wait_for_retirements`] once every window has let go.
        for tab in &mut self.window.tabs {
            tab.retire_all_shells();
        }
        Ok(())
    }

    /// **Put this window in the vault as one row** (multiwindow slice D, ruling
    /// ②).
    ///
    /// Closing a tab has always filled Recent; closing a window closes every tab
    /// in it, and one gesture that discards six tabs with no way back would be
    /// the asymmetry `seed`'s own header exists to prevent. One row rather than
    /// six, because what was lost was a window.
    ///
    /// A tab whose identity leaf seeds nothing contributes nothing, on
    /// [`TabState::seed`]'s standing rule — an unwritable row is not written
    /// rather than written as a guess — and a window that seeds nothing at all is
    /// not recorded, because an empty row would offer to reopen nothing.
    ///
    /// The pages are deliberately **not** carried. A [`seed::RecentEntry`]'s
    /// `previews` list belongs to one tab, and a window's row stands for several;
    /// flattening every tab's pages into one list would reopen the first tab
    /// holding all of them. What each tab was showing rides in its own child
    /// seed, which for a preview tab is the file itself.
    fn vault_this_window(&mut self, at: SystemTime) {
        // **The tabs it was still being asked about go in with it.** An
        // unanswered question is not a "no" (§7.1.4), and for an *open* window
        // that promise is kept by `window_snapshot` folding them back into the
        // file — but this window is leaving the file. So the row that stands for
        // it carries them, which is the only place left that can, and the
        // question loses the rows it can no longer land on.
        let pending = std::mem::take(&mut self.window.pending_restore);
        let seeds: Vec<seed::Seed> = self
            .window
            .tabs
            .iter()
            .filter_map(TabState::seed)
            .chain(pending.iter().map(restore_row_seed))
            .collect();
        self.app
            .restore_question
            .retain(|asked| !pending.contains(asked));
        if seeds.is_empty() {
            return;
        }
        self.app
            .recent
            .record(seed::Seed::Window { seeds }, Vec::new(), at);
    }
}
