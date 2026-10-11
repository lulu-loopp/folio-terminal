//! `quake` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{Runtime, native_window, persisted_window_bounds, profiles, quake, shortcuts};
use anyhow::Result;

impl Runtime<'_> {
    /// **Bring the summoned terminal down over whatever is on the screen**
    /// (§7.54).
    ///
    /// The rectangle is computed **here and now**, every time, and that is the
    /// whole difference between this door and the one above it. A window opened
    /// by a verb stands where the verb or the file said; this one stands across
    /// the top of the work area of the monitor **the pointer is on**, because
    /// that is the only thing on the desk that says which screen the reader is
    /// working on. A saved rectangle would put it on the screen they were working
    /// on yesterday — narrowed since next29 to the one rectangle a *hand* made on
    /// this very display, which is [`quake::Quake::placement`]'s own note and the
    /// one door this function asks.
    ///
    /// **The posture is re-stated.** A window that has been hidden and shown
    /// again has been through `ShowWindow` twice, and `HWND_TOPMOST` is a place
    /// in a z-order that other programs are entitled to move. Saying it again
    /// costs one `SetWindowPos` per summon and is the only thing that makes "it
    /// comes down in front" true on the twentieth press as well as the first.
    ///
    /// **Re-entrant, and that is the requirement rather than a nicety**: this
    /// runs on every press, and the three things showing a window does — two
    /// presents, the shown flag and the dpi reconciliation — were written for a
    /// door that runs once. They are re-entrant because each of them is a
    /// statement of fact rather than a step: present what is composed, say the
    /// window has been on the glass, ask Win32 which monitor it is actually on.
    #[cfg(not(target_os = "linux"))]
    pub(crate) fn show_quake_window(&mut self) -> Result<()> {
        let native = native_window(&self.window.window)?;
        #[cfg(target_os = "linux")]
        {
            let backend = crate::linux_window_backend(&self.window.window)?;
            let refusal = [
                bt_platform::linux_window::Operation::SetGlobalPosition,
                bt_platform::linux_window::Operation::Restore,
                bt_platform::linux_window::Operation::RequestFocus,
            ]
            .into_iter()
            .find_map(|operation| bt_platform::linux_window::refusal(backend, operation));
            if let Some(reason) = refusal {
                let message = format!("native Wayland summon is unavailable: {reason}");
                crate::diagnostics::note(&message);
                return Err(anyhow::anyhow!(message));
            }
            self.restore_minimized_window()?;
        }
        // **The machine is read in one place and the rules are applied in one
        // place** (§7.54e ③, user ruling 2026-09-05: 「呼出规则唯一…写成一个函数,
        // 所有入口调它」). Which display, how big on it, and whether the reader has
        // arranged this window there with their own hand are all one question, and
        // this door does not answer any part of it — it asks
        // `quake::SummonScreen::under_the_pointer` and `quake::Quake::placement`,
        // which are the whole of the answer and are pinned by the source gate
        // `a_summon_is_placed_by_one_function_and_main_does_not_do_the_geometry`.
        let screen = quake::SummonScreen::under_the_pointer(
            native,
            self.window.renderer.dpi_milli().get() * 96 / 1000,
        );
        #[cfg(target_os = "linux")]
        if screen.work.right <= screen.work.left || screen.work.bottom <= screen.work.top {
            let message = "Linux summon placement is unavailable because the display work area could not be read";
            crate::diagnostics::note(message);
            return Err(anyhow::anyhow!(message));
        }
        let settings = self.app.settings_store.loaded();
        let rect = self.app.quake.placement(&screen, settings);
        let work = screen.work;
        // One line, on `BT_TEAR_OUT`'s own terms: a summon's rectangle is a
        // function of two things read off the machine at the moment of the press,
        // and a photograph of a window in the wrong place cannot say which of
        // them was wrong. Printed once per summon.
        eprintln!(
            "BT_QUAKE work={},{} {}x{} rect={},{} {}x{}",
            work.left,
            work.top,
            work.right - work.left,
            work.bottom - work.top,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        );
        // **Placement stays on the window-owning winit thread.** X11 uses
        // `set_outer_position` after the actual backend preflight above; Windows
        // uses `stand_window_at` so its dpi-seam readback can settle the frame.
        // Wayland was refused before geometry or visibility changed.
        #[cfg(target_os = "linux")]
        self.window.window.set_outer_position(crate::WindowOrigin {
            x: rect.left,
            y: rect.top,
        });
        #[cfg(not(target_os = "linux"))]
        if let Err(error) = bt_platform::stand_window_at(native, rect) {
            eprintln!("BT_QUAKE {error}");
        }
        #[cfg(target_os = "linux")]
        self.window
            .window
            .set_window_level(winit::window::WindowLevel::AlwaysOnTop);
        #[cfg(not(target_os = "linux"))]
        if let Err(error) = bt_platform::set_window_topmost(native, true) {
            eprintln!("recoverable always-on-top failure: {error}");
        }
        // Never maximized: this window's shape *is* the rectangle above, and a
        // maximize would throw it away for the one Windows keeps.
        let first_time = !self.window.window_shown;
        self.put_the_window_on_the_glass(false)?;
        // The pages of whatever tabs it opened holding, on the launch door's own
        // terms and for its reason — and only on the summon that first put this
        // window on the glass, because a controller composes against a window
        // that has been shown and there has now been one.
        if first_time {
            self.revive_all_web_pages()?;
        }
        Ok(())
    }

    /// Ask the display worker about the screen selected by the X11 key event.
    #[cfg(target_os = "linux")]
    pub(crate) fn request_quake_screen(
        &mut self,
        pointer: Option<(i32, i32)>,
    ) -> Result<bt_platform::linux_display::LinuxDisplayRequest> {
        let native = native_window(&self.window.window)?;
        let backend = crate::linux_window_backend(&self.window.window)?;
        let refusal = [
            bt_platform::linux_window::Operation::SetGlobalPosition,
            bt_platform::linux_window::Operation::Restore,
            bt_platform::linux_window::Operation::RequestFocus,
        ]
        .into_iter()
        .find_map(|operation| bt_platform::linux_window::refusal(backend, operation));
        if let Some(reason) = refusal {
            let message = format!("native Wayland summon is unavailable: {reason}");
            crate::diagnostics::note(&message);
            return Err(anyhow::anyhow!(message));
        }
        let cached_dpi = self.window.renderer.dpi_milli().get() * 96 / 1000;
        let query = match pointer {
            Some((x, y)) => bt_platform::linux_display::LinuxDisplayQuery::SummonScreenAt {
                window: native,
                cached_dpi,
                x,
                y,
            },
            None => bt_platform::linux_display::LinuxDisplayQuery::SummonScreen {
                window: native,
                cached_dpi,
            },
        };
        let generation = self.app.next_display_generation();
        bt_platform::linux_display::request_display(
            u64::from(self.window.window.id()),
            generation,
            query,
        )
        .map_err(|error| anyhow::anyhow!(error))
    }

    /// Place and show the summoned window after its display facts arrive.
    #[cfg(target_os = "linux")]
    pub(crate) fn show_quake_window_at(&mut self, screen: quake::SummonScreen) -> Result<()> {
        if screen.work.right <= screen.work.left || screen.work.bottom <= screen.work.top {
            let message = "Linux summon placement is unavailable because the display work area could not be read";
            crate::diagnostics::note(message);
            return Err(anyhow::anyhow!(message));
        }
        let settings = self.app.settings_store.loaded();
        let rect = self.app.quake.placement(&screen, settings);
        let work = screen.work;
        eprintln!(
            "BT_QUAKE work={},{} {}x{} rect={},{} {}x{}",
            work.left,
            work.top,
            work.right - work.left,
            work.bottom - work.top,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        );
        self.window.window.set_outer_position(crate::WindowOrigin {
            x: rect.left,
            y: rect.top,
        });
        self.window
            .window
            .set_window_level(winit::window::WindowLevel::AlwaysOnTop);
        let first_time = !self.window.window_shown;
        self.put_the_window_on_the_glass(false)?;
        if first_time {
            self.revive_all_web_pages()?;
        }
        Ok(())
    }

    /// **Send it back up** (§7.54), and say who is owed the keyboard.
    ///
    /// `set_visible(false)` and not a close: the window keeps its tabs, its
    /// shells and its scrollback, which is what makes the next summon instant and
    /// what lets a build keep running in it while nobody is looking.
    ///
    /// `window_shown` is deliberately **not** cleared. It records that this
    /// window has been on the glass at least once — which is what the first
    /// visible present's dpi check reads — and a hidden window is not a window
    /// that was never shown.
    pub(crate) fn hide_quake_window(&mut self) -> Option<bt_platform::hotkey::Foreground> {
        #[cfg(target_os = "linux")]
        match crate::linux_window_backend(&self.window.window) {
            Ok(bt_platform::linux_window::Backend::X11) => {}
            Ok(bt_platform::linux_window::Backend::Wayland) => {
                crate::diagnostics::note(
                    "native Wayland summon was left visible because restore and activation are unsupported",
                );
                return None;
            }
            Err(error) => {
                crate::diagnostics::note(&format!(
                    "summoned window was left visible because its Linux backend is unknown: {error}"
                ));
                return None;
            }
        }
        // An owner-thread door (`doors::SetVisible`, whose station the meter enters). A refusal
        // is a hide that had no effect.
        let _ = bt_platform::admission::admitted::<bt_platform::admission::doors::SetVisible, _>(
            |token| crate::owner_door::set_visible(token, &self.window.window, false),
        );
        self.app.quake.hidden()
    }

    /// **Whether this window is the one a key summons** (§7.54).
    ///
    /// Asked of the application rather than answered by a flag of this window's,
    /// because there is exactly one summoned window per process and the field
    /// that names it is the same field the door, the press and the row read. Two
    /// copies of one identity is two chances for a window to disagree about what
    /// it is.
    pub(crate) fn is_quake_window(&self) -> bool {
        self.app.quake.is_quake(self.window.window.id())
    }

    /// **Which line of the shortcut table is the summon** (§7.54e ⑤).
    ///
    /// By the row's id and not by ordinal, for `SettingsTarget::Record`'s own reason read the other
    /// way: the index is a fact about the list drawn this frame, so it has to be derived from that
    /// list rather than written down anywhere. `None` on a build whose table has no such row, which
    /// is the honest answer for a press that then does nothing — and it is the same lookup the
    /// dialog's own caps come through, so the box a press opens is the box it was drawn on.
    pub(crate) fn summon_shortcut_line(&self) -> Option<usize> {
        shortcuts::summon_line(&self.app.shortcuts.editor_rows())
    }

    /// Whether the summoned terminal goes away when the keyboard leaves it
    /// (§7.54).
    ///
    /// The plainest applier in this file, and deliberately: there is nothing to
    /// say to the window. The switch is read at the moment a blur is spent
    /// (`FolioApp::settle_quake`), so turning it off while the window is up
    /// leaves the window up, and turning it on does not send away a window whose
    /// keyboard left before the row was pressed. A row that reached back and
    /// acted on a blur that had already been read would be answering a question
    /// nobody asked twice.
    pub(crate) fn apply_quake_dismiss(&mut self, enabled: bool) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.quake_dismiss_on_blur = enabled;
        if &settings == self.app.settings_store.loaded() {
            return Ok(false);
        }
        Ok(self.app.settings_store.store(settings))
    }

    /// **How much of the summoned terminal a new run puts back** (§7.54e ④).
    ///
    /// [`Self::apply_quake_dismiss`]'s shape and its reason: there is nothing to say to a window.
    /// What this row governs happens at the *next* launch, in `plan_windows`, which reads the
    /// stored rung — so a reader who changes it is changing what tomorrow looks like and this turn
    /// has nothing to do but write it down.
    pub(crate) fn apply_quake_restore(&mut self, rung: bt_persist::QuakeRestoreV1) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.quake_restore = rung;
        if &settings == self.app.settings_store.loaded() {
            return Ok(false);
        }
        Ok(self.app.settings_store.store(settings))
    }

    /// **Which profile the summoned terminal opens on** (§7.54e ⑤).
    ///
    /// `None` is the picker's first item and is stored as the empty string, which is
    /// `bt_persist`'s own word for "whatever the default profile is" — the same value the row on
    /// `General` uses for the same sentence, so the two cannot come to mean different things.
    ///
    /// **Nothing is said to a window that is already open**: a profile is what a *new* tab starts
    /// as, which is exactly what the `Default profile` row above it does and does not do.
    pub(crate) fn apply_quake_profile(&mut self, profile: Option<usize>) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.quake_profile_id = profile.map_or_else(
            || bt_persist::DEFAULT_PROFILE_UNSET.to_owned(),
            profiles::id,
        );
        if &settings == self.app.settings_store.loaded() {
            return Ok(false);
        }
        Ok(self.app.settings_store.store(settings))
    }

    /// **The command the first summon of a run writes into the window** (§7.54e ⑤).
    ///
    /// Stored on the keystroke, which is this dialog's own rule (§7.1.6c-4a: there is no commit and
    /// nothing to save) — and it is safe to store on the keystroke precisely because storing is all
    /// this does. The command is spent by `FolioApp::summon_quake`, once a launch, through
    /// `quake::Quake::take_startup_command`; a half-typed row is a row that has not been summoned
    /// against yet.
    pub(crate) fn apply_quake_command(&mut self, command: String) -> Result<bool> {
        let mut settings = self.app.settings_store.loaded().clone();
        settings.quake_startup_command = command;
        if &settings == self.app.settings_store.loaded() {
            return Ok(false);
        }
        Ok(self.app.settings_store.store(settings))
    }

    /// **File the summoned window's rectangle under the display it is on, but
    /// only when a hand put it there** (§7.54, user ruling next29).
    ///
    /// The whole of the judgement is `in_size_move`, and it is the same judgement
    /// the divider drag is read with: Windows sets it between `WM_ENTERSIZEMOVE`
    /// and `WM_EXITSIZEMOVE`, which is exactly the interval a person is holding
    /// the frame. Every rectangle this program states for itself — the summon's
    /// own `stand_window_at`, a restore, a dpi settlement — arrives outside that
    /// interval, so none of them is mistaken for a preference. That is why there
    /// is no "we did this ourselves" flag to keep in step: the question is not
    /// *who called `SetWindowPos`*, it is *whose hand was on the window*, and
    /// Windows is already answering it.
    ///
    /// It runs on the moves and resizes of a drag rather than at its end, so the
    /// last one to arrive is what is kept. There is no cheaper moment: an
    /// `WM_EXITSIZEMOVE` is not a winit event, and a rectangle read once a frame
    /// while a window is being dragged is one `GetWindowRect` against a drag that
    /// is already repainting the screen.
    pub(in crate::runtime) fn remember_summoned_arrangement(&mut self) {
        if !self.is_quake_window() || !self.window.custom_window_frame.in_size_move() {
            return;
        }
        #[cfg(target_os = "linux")]
        {
            if self.window.pending_summoned_arrangement.is_some() {
                self.window.summoned_arrangement_refresh_owed = true;
                return;
            }
            self.queue_summoned_arrangement();
        }
        #[cfg(not(target_os = "linux"))]
        {
            let Ok(native) = native_window(&self.window.window) else {
                return;
            };
            let Ok(rect) = bt_platform::get_window_rect(native) else {
                return;
            };
            let Some(monitor) = bt_platform::monitor_id_at(rect.left, rect.top) else {
                return;
            };
            let dpi = bt_platform::dpi_at(rect.left, rect.top);
            let bounds = persisted_window_bounds(rect, f64::from(dpi.max(1)) / 96.0);
            self.app.quake.remember(monitor, bounds);
        }
    }

    #[cfg(target_os = "linux")]
    fn queue_summoned_arrangement(&mut self) {
        let Ok(native) = native_window(&self.window.window) else {
            return;
        };
        let generation = self.app.next_display_generation();
        if let Ok(request) = bt_platform::linux_display::request_display(
            u64::from(self.window.window.id()),
            generation,
            bt_platform::linux_display::LinuxDisplayQuery::SummonedArrangement { window: native },
        ) {
            self.window.pending_summoned_arrangement = Some(request);
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn apply_linux_display_ready(
        &mut self,
        ready: bt_platform::linux_display::LinuxDisplayReady,
    ) -> Result<bool> {
        let matches = self
            .window
            .pending_summoned_arrangement
            .as_ref()
            .is_some_and(|request| request.ready() == ready);
        if !matches {
            return Ok(false);
        }
        let Some(request) = self.window.pending_summoned_arrangement.take() else {
            return Ok(false);
        };
        let answer = request.try_take();
        if std::mem::take(&mut self.window.summoned_arrangement_refresh_owed) {
            if self.is_quake_window() {
                self.queue_summoned_arrangement();
            }
            return Ok(true);
        }
        if let Ok(bt_platform::linux_display::LinuxDisplayAnswer::SummonedArrangement(Some((
            rect,
            monitor,
            dpi,
        )))) = answer
        {
            let bounds = persisted_window_bounds(rect, f64::from(dpi.max(1)) / 96.0);
            self.app.quake.remember(monitor, bounds);
        }
        Ok(true)
    }
}
