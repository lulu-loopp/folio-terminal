//! `diagnostics` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::window_seam::prelude::*;
use crate::{
    ApplicationChange, GridChange, LeafOnStage, Runtime, card_trace, coalesce, resolve_theme_mode,
    system_os_theme,
};
use anyhow::Result;
use bt_persist::ThemeModeV1;
use bt_render::{GridSize, SystemContrast, Theme, ThemeChange};
use std::time::Instant;
use winit::dpi::PhysicalSize;
use winit::window::Theme as OsTheme;

impl<B: Backend> Runtime<'_, B> {
    /// Windows changed its mind about light and dark.
    ///
    /// The event is the **trigger**; the answer comes from [`system_os_theme`],
    /// the same reader the boot used. winit hands a theme along with the
    /// notification and it is deliberately not taken: two readers of one machine
    /// setting are two chances to disagree about it, and the one this process
    /// already trusts is the one that decided the canvas its window opened in.
    pub(crate) fn os_theme_changed(&mut self) -> Result<bool> {
        let mode = self.app.settings_store.loaded().theme_mode;
        // **A contrast theme decides the canvas while it is on** (owner ruling 2026-10-28,
        // row 90 ⑥), whatever the Theme row says and whatever Windows' light/dark switch
        // says beside it.
        if bt_render::high_contrast_in_force().is_some() {
            return self.apply_theme(theme_in_force(mode));
        }
        let Some(os_theme) = system_os_theme() else {
            return Ok(false);
        };
        let Some(theme) = resolved_theme_change(mode, os_theme) else {
            return Ok(false);
        };
        self.apply_theme(theme)
    }

    /// **A reading of the Windows contrast theme, put in force** (T-HIGH-CONTRAST,
    /// `docs/DESIGN.md` §7.1.6: 「高对比度模式跟随系统调色」) — at start, and on every
    /// preference notice with the system canvas beside it.
    ///
    /// `set_high_contrast` answers `Unchanged` when the reading is the one in force, which is
    /// every notice but the one that moved it, and every window after the first: the overlay
    /// is the process's, so the first window to hear the change puts it in force, re-derives,
    /// and records the change for the others ([`Self::adopt_new_palette`]).
    ///
    /// What moves with it, in this window: the ground (no picture and no translucency while a
    /// theme is on, ruling ⑦), the canvas (ruling ⑥), and the palette, which every one of the
    /// three re-derives through the one [`Self::adopt_new_palette`].
    pub(crate) fn adopt_high_contrast(&mut self, reading: Option<SystemContrast>) -> Result<bool> {
        if bt_render::set_high_contrast(reading) == ThemeChange::Unchanged {
            return Ok(false);
        }
        let ground_moved = self.apply_window_ground()?;
        let theme_moved = self.apply_theme(theme_in_force(
            self.app.settings_store.loaded().theme_mode,
        ))?;
        if !ground_moved && !theme_moved {
            self.adopt_new_palette()?;
        }
        Ok(true)
    }

    /// Record that the application moved under its windows. See
    /// [`App::pending_application_change`].
    pub(crate) fn note_application_change(&mut self, change: ApplicationChange) {
        self.app.pending_application_change =
            Some(change.merged_with(self.app.pending_application_change));
    }

    /// **Re-derive whatever the application changed** (multiwindow slice C).
    ///
    /// Called on the windows that were not the one the verb ran in. Each half is
    /// the same call that window made for itself, which is what keeps this from
    /// being a second, drifting answer to "what does a face change cost".
    pub(crate) fn adopt_application_change(&mut self, change: ApplicationChange) -> Result<()> {
        if change.font {
            self.adopt_terminal_font()?;
        }
        if change.look {
            self.adopt_new_palette()?;
        }
        // **A caret change costs a frame, and the two branches above each end in
        // one.** The shape is read at draw time out of the process static, so a
        // frame published for a new face or a new palette already has the new
        // caret in it; asking for a second would compose the same picture twice
        // in one round. This is the one place the flags interact, and it is
        // stated rather than relied on: neither branch above may stop
        // publishing without this line being read again.
        if change.caret && !(change.font || change.look) {
            self.adopt_new_cursor_style()?;
        }
        // **And this one costs no frame at all**, which is why it is outside the
        // interaction above: it changes what a *key* will mean on this window
        // and nothing that is on the glass. See `apply_option_sends_alt`.
        if change.option {
            self.adopt_option_as_alt();
        }
        Ok(())
    }

    /// One line per drain turn, and only when `BT_PERF_TRACE` asked for one.
    ///
    /// The publication rule is otherwise invisible: its whole effect is a frame that did not
    /// happen, which no existing line reports. The owner's trace runs already record every
    /// `read(2)` boundary and arrival in the `BT_PTY_DUMP` `.chunks` sidecar, so a run traced
    /// with both can be replayed against its own recording and every decision here checked.
    pub(in crate::runtime) fn trace_drain(
        &self,
        slices: usize,
        bytes: usize,
        arrival: &coalesce::Arrival,
        decision: coalesce::Publication,
        opened: Instant,
        now: Instant,
    ) {
        if !self.app.trace_perf {
            return;
        }
        let (publish, deferred_us, reason) = match decision {
            coalesce::Publication::Now => (
                "now",
                0,
                if arrival.sync_open {
                    "sync-block"
                } else if arrival.sync_closed {
                    "sync-commit"
                } else if arrival.ring_pending {
                    "ring-pending"
                } else if arrival.ends_capped {
                    "flood-bound"
                } else {
                    "short-read"
                },
            ),
            coalesce::Publication::WaitUntil(until) => (
                "deferred",
                until.saturating_duration_since(now).as_micros(),
                "capped",
            ),
        };
        crate::diagnostics::stderr_line(format!(
            "BT_PERF_TRACE drain slices={slices} bytes={bytes} capped={} ring_pending={} sync_open={} \
             sync_closed={} owed_us={} publish={publish} deferred_us={deferred_us} reason={reason}",
            arrival.ends_capped,
            arrival.ring_pending,
            arrival.sync_open,
            arrival.sync_closed,
            now.saturating_duration_since(opened).as_micros(),
        ));
    }

    /// Carry a freshly solved grid to the child and to our own grid.
    ///
    /// The seat rectangle has already moved by the time this is called (`resolve_seat_layout` is
    /// what produced `next_grid`), and our own actor follows it in the same turn; only the child's
    /// hearing of it is coalesced, at the 200 ms quiet boundary every `Resized` shares. A grid that
    /// is wider than its seat is the ordinary case for the renderer in between — the seat viewport
    /// scissors it — and it is the case a divider drag produces on every frame.
    pub(in crate::runtime) fn schedule_grid_change(
        &mut self,
        next_grid: GridSize,
        physical: PhysicalSize<u32>,
        observed_at: Instant,
        context: &'static str,
    ) -> Result<()> {
        let active = self.window.active_tab;
        // **Which pane this is**, read before the `&mut` below (`BT_CARD_TRACE`).
        let pane = card_trace::Pane {
            window: u64::from(self.window.window.id()),
            tab: self.window.tabs[active].id,
            seat: self.window.tabs[active].focused_leaf,
        };
        // A tab with no shell has no grid to carry anywhere and no child to
        // carry it to (§7.1.6h). The rectangle it was solved into is real and
        // the chrome uses it; what is absent is the *cell* reading of that
        // rectangle, which is a fact about a terminal.
        let Some(leaf) = self.window.tabs[active].focused_mut() else {
            return Ok(());
        };
        leaf.propose_size(GridChange {
            next: next_grid,
            physical,
            observed_at,
            on_stage: LeafOnStage::Shown,
            context,
            pane,
        })?;
        Ok(())
    }
}

/// **The canvas a window resolves to, with the contrast theme in it** — every resolution
/// after the boot's first reading of the machine goes through here, so the Theme row, the
/// desktop's light/dark switch and a contrast theme cannot disagree about one window.
pub(crate) fn theme_in_force(mode: ThemeModeV1) -> Theme {
    theme_under_contrast(
        resolve_theme_mode(mode, system_os_theme()),
        bt_render::high_contrast_in_force(),
    )
}

/// **A contrast theme's canvas outranks the Theme row while it is on** (owner ruling
/// 2026-10-28, row 90 ⑥): the theme's Background decides light or dark at the one threshold,
/// and `resolved` (the row's own answer) is the canvas again the moment it goes off. The row
/// keeps its value throughout, because nothing here writes it.
pub(crate) fn theme_under_contrast(resolved: Theme, contrast: Option<SystemContrast>) -> Theme {
    contrast.map_or(resolved, |system| {
        if bt_render::background_is_light(system.window) {
            Theme::Light
        } else {
            Theme::Dark
        }
    })
}

/// **What this machine's contrast theme is right now**, in the renderer's vocabulary, or
/// `None` while none is on.
///
/// Windows alone has one (`bt_platform::system_high_contrast`). A Mac's Increase Contrast is
/// an accessibility display option of another kind and is not read here, so every other
/// platform answers `None` and the overlay is never set there.
#[cfg(windows)]
pub(crate) fn read_high_contrast() -> Option<SystemContrast> {
    bt_platform::system_high_contrast().map(|colours| SystemContrast {
        window: colours.window,
        window_text: colours.window_text,
        highlight: colours.highlight,
        highlight_text: colours.highlight_text,
        gray_text: colours.gray_text,
    })
}

/// See the Windows arm: no other platform has a contrast theme to read.
#[cfg(not(windows))]
pub(crate) fn read_high_contrast() -> Option<SystemContrast> {
    None
}

pub(crate) fn resolved_theme_change(mode: ThemeModeV1, os_theme: OsTheme) -> Option<Theme> {
    match mode {
        ThemeModeV1::System => Some(resolve_theme_mode(mode, Some(os_theme))),
        ThemeModeV1::Light | ThemeModeV1::Dark => None,
    }
}
