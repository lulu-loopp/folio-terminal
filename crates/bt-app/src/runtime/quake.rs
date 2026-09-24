//! `quake` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    Runtime, hang_watch, native_window, persisted_window_bounds, profiles, quake, shortcuts,
};
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
    pub(crate) fn show_quake_window(&mut self) -> Result<()> {
        let native = native_window(&self.window.window)?;
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
        // **`stand_window_at` and not `set_window_outer_rect`** (§7.54, measured
        // 2026-09-02). Every other caller places a window on the monitor it is
        // already on; a summon routinely names a different one, and across a dpi
        // seam a single `SetWindowPos` is not a move but the thing that raises
        // `WM_DPICHANGED` — after which §7.50's ruling hands the rectangle to
        // Windows for the length of that message and the summon's own request is
        // resolved onto the system's suggestion. See that function for the two
        // measurements and for why saying it again is the fix.
        //
        // A rectangle that would not be taken is said out loud and the window is
        // shown anyway: it is standing somewhere, and a summon a few pixels off is
        // enormously better than a summon that refused to come down.
        if let Err(error) = bt_platform::stand_window_at(native, rect) {
            eprintln!("BT_QUAKE {error}");
        }
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
        hang_watch::during(hang_watch::Station::WindowVisible, || {
            self.window.window.set_visible(false)
        });
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
        self.app
            .shortcuts
            .editor_rows()
            .iter()
            .position(|line| line.ids.contains(&shortcuts::SUMMON_QUAKE_ID))
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
        let Ok(native) = native_window(&self.window.window) else {
            return;
        };
        let Ok(rect) = bt_platform::get_window_rect(native) else {
            return;
        };
        // The display the window is on *now*, asked at its own top-left rather
        // than at the pointer: a person dragging a window across a seam has the
        // pointer on the display they are dragging towards while most of the
        // window is still on the one they are leaving, and the answer wanted is
        // where the window came to rest.
        let Some(monitor) = bt_platform::monitor_id_at(rect.left, rect.top) else {
            return;
        };
        let dpi = bt_platform::dpi_at(rect.left, rect.top);
        let bounds = persisted_window_bounds(rect, f64::from(dpi.max(1)) / 96.0);
        self.app.quake.remember(monitor, bounds);
    }
}
