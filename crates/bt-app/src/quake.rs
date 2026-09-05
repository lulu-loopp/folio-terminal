//! **The terminal a key calls down over whatever is on the screen**
//! (`docs/DESIGN.md` §7.54).
//!
//! One window of this process, in every way that matters: it has its own tabs
//! and panes, it goes into `session.json` beside the others, and every verb that
//! works in a window works in it. What is different is three facts, and this
//! module is the three of them written down in one place.
//!
//! **It is asked for from outside.** The chord is claimed from Windows with
//! `RegisterHotKey` rather than matched against keys this process was handed,
//! because the whole point of the window is to arrive while some *other* program
//! has the keyboard. See [`bt_platform::hotkey`] for that half.
//!
//! **Its rectangle is computed and never remembered.** Every summon reads the
//! work area of the monitor the pointer is on and takes its share of the top of
//! it, centred — see [`summoned_rect`]. The window's saved placement is written into the document
//! like every other window's and is deliberately not read back: a person with two
//! screens summons the terminal on whichever one they are working on, and a
//! rectangle out of a file would put it on the one they were working on
//! yesterday.
//!
//! **It is hidden rather than closed.** Between summons the window is a live
//! window with `set_visible(false)` on it, which is what makes the second summon
//! instant and what lets a shell keep running in it while nobody is looking. It
//! is therefore also *born* hidden on a restore, and it takes no part in the
//! restore prompt's election — a question about a window nobody can see is a
//! question nobody can answer.

use std::num::NonZeroIsize;

use bt_platform::WindowRect;
use bt_platform::hotkey::{GlobalHotkey, Hotkey, HotkeyFault};
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

use crate::shortcuts::Chord;

/// **The id this process claims its one chord under.**
///
/// A `RegisterHotKey` id is unique per *thread*, and this program registers on
/// the event loop's thread and nowhere else, so one constant is the whole
/// namespace. It is what [`bt_platform::hotkey::is_our_hotkey`] is asked about,
/// and it is not `0` because a `wparam` of zero is the one value a message that
/// carries nothing would also have.
pub(crate) const SUMMON_HOTKEY_ID: i32 = 1;

/// **Where a summon puts the window** — pure, and the whole of the geometry.
///
/// The work area and not the monitor: the taskbar is the one thing on the screen
/// that is *also* always in front, and a terminal spanning the top of a display
/// whose taskbar is docked there would open underneath it.
///
/// **Two percentages of that work area, and the second of them is a ruling**
/// (user, 2026-09-02). Until v28 the left and right were the work area's own and
/// this note said there was no width setting and never would be; a 4K ultrawide
/// answered that, and the shape is now the reader's percentage of the width,
/// **centred in what it does not cover**. Centred and not left-aligned, because
/// the window arrives over whatever the reader was already looking at and the
/// middle of the screen is where they were already looking; a strip pinned to one
/// edge is a strip they have to find. The height is the reader's percentage of
/// the work area's height, hung from its top, which is where a key-summoned
/// terminal has come down since the shape was invented.
///
/// Both percentages are clamped into the range their row offers, so that a
/// hand-edited `settings.json` saying `400` opens a full-size window rather than
/// one four screens across, and one saying `0` opens a usable window rather than a
/// line. The clamp is here and not at the file's door because `bt_persist`
/// deliberately stores what it was given (see `SettingsV1::quake_height` and
/// `SettingsV1::quake_width`), and this is the surface that has to place the
/// window.
///
/// **The odd pixel goes to the right.** A centred window inside an odd remainder
/// cannot have two equal margins, and the whole of the guarantee is that neither
/// edge leaves the work area: the left margin is half the remainder rounded down,
/// which puts the leftover column on the right and keeps `right <= work.right` on
/// every input rather than only on even ones.
///
/// **And it does not touch the top of the work area** (user ruling, next29). It
/// hangs `top_gap` logical pixels below it, for a reason that is only visible on a
/// real screen: this window is drawn with rounded corners, and a rounded window
/// flush against the top of a display shows two square notches where the corners
/// stop being the window and the desktop behind is not yet the desktop. The gap
/// is what makes it read as a panel that has come down over the screen rather
/// than as a window that has been cropped by it, and it is the same gap the
/// focused search box floats on.
///
/// **The gap is logical pixels and the rectangle is physical ones**, so the dpi
/// of the monitor being summoned onto is a parameter rather than something this
/// function could read: it is placing a window on a display that this process
/// may not have a window on yet, and the scale of the one it *does* have a
/// window on is the wrong number. [`bt_platform::logical_px_for_dpi`] is the
/// same converter `tear_out_rect` uses for the same reason.
///
/// **The height does not shrink to pay for the gap** — the window is moved down,
/// not cropped. The reader's percentage answers "how much of this screen", and a
/// percentage that quietly returned twelve pixels less than it said would be a
/// number that means something different at every dpi. The consequence is stated
/// rather than hidden: at a full hundred percent the bottom edge now lands
/// `top_gap` below the work area, over whatever is docked there.
///
/// **The gap is a row and no longer a constant** (user ruling, 2026-09-05, §7.54e): next29 wired
/// twelve into this file, and the reader who wants the panel flush against the edge of the screen —
/// or further down it — now has a number to say so with. It is clamped here for the reason both
/// percentages are: `bt_persist` stores what it was given, and this is the surface that has to
/// place the window. There is no floor, because zero is a shape somebody may want and is the shape
/// this window had before the gap was invented.
#[must_use]
pub(crate) fn summoned_rect(
    work: WindowRect,
    width_percent: u8,
    height_percent: u8,
    top_gap: u32,
    dpi: u32,
) -> WindowRect {
    let vertical = height_percent.clamp(bt_persist::MINIMUM_QUAKE_HEIGHT, 100);
    let horizontal = width_percent.clamp(bt_persist::MINIMUM_QUAKE_WIDTH, 100);
    let gap = top_gap.min(bt_persist::MAXIMUM_QUAKE_TOP_GAP);
    let tall = work.bottom.saturating_sub(work.top).max(1);
    let wide = work.right.saturating_sub(work.left).max(1);
    // In `i64`, because a work area on a wall of displays is allowed to be big
    // enough that `size * 100` is not an `i32` — and a rectangle that wrapped
    // would put the window somewhere no monitor is.
    let height = (i64::from(tall) * i64::from(vertical) / 100).max(1);
    let height = i32::try_from(height).unwrap_or(tall);
    let width = (i64::from(wide) * i64::from(horizontal) / 100).max(1);
    let width = i32::try_from(width).unwrap_or(wide);
    let left = work.left.saturating_add((wide - width) / 2);
    let top = work
        .top
        .saturating_add(bt_platform::logical_px_for_dpi(gap, dpi));
    WindowRect {
        left,
        top,
        right: left.saturating_add(width),
        bottom: top.saturating_add(height),
    }
}

/// **A remembered rectangle restated in the pixels a window is placed with.**
///
/// The inverse of the snapshot's logical reading, narrowed to this one caller and
/// pure so a test can hold it without a display: the file stores logical pixels
/// because that is the unit that means the same thing at every scale, and
/// `stand_window_at` takes physical ones.
///
/// The size floors at one pixel each way. A rectangle of no width is not a
/// window a reader could find again, and the only way to reach one is a document
/// somebody edited by hand — which `bt_persist` stores as it was given by
/// design, leaving the surface that places the window owing the floor, exactly as
/// [`summoned_rect`]'s own clamps do.
#[must_use]
fn physical_rect(bounds: bt_persist::WindowBoundsV1, dpi: u32) -> WindowRect {
    let physical = |logical: i64| -> i32 {
        i32::try_from(logical * i64::from(dpi.max(1)) / 96).unwrap_or(i32::MAX)
    };
    let left = physical(i64::from(bounds.x));
    let top = physical(i64::from(bounds.y));
    let width = physical(i64::from(bounds.width)).max(1);
    let height = physical(i64::from(bounds.height)).max(1);
    WindowRect {
        left,
        top,
        right: left.saturating_add(width),
        bottom: top.saturating_add(height),
    }
}

/// **What a remembered or configured command becomes on the way to a shell** (§7.54e ④ and ⑤).
///
/// Two callers, one function, and the parameter is the whole of the difference between them — which
/// is the ruling itself made into a type rather than into two similar functions somebody can copy
/// wrong:
///
/// * **A restored command is typed and never submitted** (`submit: false`). 「绝不自动执行任何
///   命令」: what a pinned tab last ran came out of a document, and a document is a record of
///   something that happened once. It arrives standing at the prompt, and `Enter` is the reader's.
/// * **The startup command is run** (`submit: true`). It came out of a settings row whose own
///   sentence says it will be, every launch, and the reader wrote it there.
///
/// **Every control character goes, `\r` and `\n` first**, and that is not tidying: a submission is
/// exactly one byte, and a remembered command that arrived out of a hand-edited `session.json`
/// carrying a newline would submit itself — turning the whole of the ruling above into a rule that
/// holds only for well-formed documents. So the newline is not something the text may contain; it
/// is something this function adds, once, when it was asked to.
///
/// Nothing at all for a command with no characters left in it, so an empty row and a document field
/// full of control bytes are the same case: there was nothing to type.
#[must_use]
pub(crate) fn typed_into_a_prompt(command: &str, submit: bool) -> Vec<u8> {
    let typed: String = command
        .chars()
        .filter(|glyph| !glyph.is_control())
        .collect();
    if typed.trim().is_empty() {
        return Vec::new();
    }
    let mut bytes = typed.into_bytes();
    if submit {
        // `\r` and not `\n`, which is what a keyboard sends and what every shell
        // behind a ConPTY reads as "the line is finished".
        bytes.push(b'\r');
    }
    bytes
}

/// **The screen a summon is about to happen on**, read off the machine at the moment of the press.
///
/// Three facts and one question each, and they are one struct because they must all be answered
/// about the **same** display: the work area the rectangle is measured against, the dpi its gap is
/// scaled at, and the name the reader's own arrangement is filed under. Asked separately they can
/// disagree — the pointer moves between two reads — and a summon that measured one display and
/// filed under another would hand back somebody else's rectangle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SummonScreen {
    /// The work area, and not the monitor: the taskbar is the one thing on the screen that is
    /// *also* always in front.
    pub(crate) work: WindowRect,
    /// What Windows calls this display, or `None` on a machine that will not say. Nothing is
    /// remembered without a name — see [`Quake::placement_on`].
    pub(crate) monitor_id: Option<String>,
    /// The dpi of **this** display, which is not the window's cached scale: between summons the
    /// window is parked on whichever display it last came down on.
    pub(crate) dpi: u32,
}

impl SummonScreen {
    /// **The display the reader is working on**, which is the display the pointer is on.
    ///
    /// The pointer and not the window, because the pointer is the only thing on the desk that says
    /// which screen the person is at. When Windows will not say where the pointer is, the window's
    /// own display answers, and when it will not say that either the virtual screen does — in that
    /// order, because each fallback is one step further from the question actually asked.
    ///
    /// This is the machine-reading half of the one summon door; the deciding half is
    /// [`Quake::placement`], which is pure and is where the rules live.
    #[must_use]
    pub(crate) fn under_the_pointer(window: NonZeroIsize, cached_dpi: u32) -> Self {
        let pointer = bt_platform::pointer_position();
        let work = pointer
            .and_then(|(x, y)| bt_platform::work_area_at(x, y).ok())
            .or_else(|| bt_platform::get_work_area(window).ok())
            .unwrap_or_else(bt_platform::virtual_screen_rect);
        Self {
            work,
            monitor_id: pointer.and_then(|(x, y)| bt_platform::monitor_id_at(x, y)),
            dpi: pointer.map_or(cached_dpi, |(x, y)| bt_platform::dpi_at(x, y)),
        }
    }
}

/// **A chord as Windows will be asked for it**, or `None` when this layout has no
/// key for it.
///
/// The virtual key comes from `webhost::chord_virtual_key`, which is the same
/// question a page's accelerator asks and is answered by the layout actually
/// installed — a chord recorded as `` ` `` is `VK_OEM_3` on a US keyboard and
/// something else elsewhere, and the person pressing it is the one whose layout
/// counts. One reader for both, because a summon claimed on a different virtual
/// key than the one a page hands back would be two opinions about one chord.
#[must_use]
pub(crate) fn hotkey_for(chord: &Chord) -> Option<Hotkey> {
    Some(Hotkey {
        ctrl: chord.modifiers.contains(ModifiersState::CONTROL),
        alt: chord.modifiers.contains(ModifiersState::ALT),
        shift: chord.modifiers.contains(ModifiersState::SHIFT),
        win: chord.modifiers.contains(ModifiersState::SUPER),
        virtual_key: crate::webhost::chord_virtual_key(&chord.key)?,
    })
}

/// **Everything this process knows about the summoned terminal**, held on `App`
/// because a chord is claimed once per *process* and the window it calls up is
/// one of the windows in the map beside it.
#[derive(Default)]
pub(crate) struct Quake {
    /// The live claim on the chord. Dropping it releases the key, which is what
    /// happens when the reader rebinds or clears the row.
    claim: Option<GlobalHotkey>,
    /// The chord [`Self::claim`] was made for, so a turn can tell "the reader
    /// changed the key" from "nothing happened" without asking Windows.
    ///
    /// Held even when the claim was refused: what it records is what was *asked
    /// for*, and a reconciliation that forgot it would re-ask Windows for a key
    /// it has already said no to, once per turn of the loop, for ever.
    claimed_for: Option<Chord>,
    /// Why the last claim failed, or `None` when there was nothing to claim or
    /// the claim stands. Read by the settings page — see
    /// `settings::SettingsValues::quake_hotkey_taken`.
    fault: Option<HotkeyFault>,
    /// The window, once there is one. `None` before the first summon on a launch
    /// with nothing saved.
    window: Option<WindowId>,
    /// Whether that window is on the screen right now.
    ///
    /// Held here rather than asked of the window, because the question "is the
    /// summon showing" is asked on turns when the window may not be borrowable
    /// and is answered by the same field that decides which way the next press
    /// goes. It is written at exactly the two places that show and hide it.
    shown: bool,
    /// **Who had the keyboard before the summon**, so that dismissing can give it
    /// back.
    ///
    /// Read once, at the moment of showing, and spent once, at the moment of
    /// hiding. There is no second chance to read it: by the time the window is
    /// going away the foreground is the window.
    give_back: Option<NonZeroIsize>,
    /// **A press that has arrived and not yet been acted on.**
    ///
    /// The message hook does nothing but wake the loop (see the hook's own note
    /// in `main`), so what a `WM_HOTKEY` leaves behind is this bit and a turn.
    summoned: bool,
    /// **The window lost the keyboard and the reader wants it gone.**
    ///
    /// Set by the blur and spent on the *next* turn rather than in the arm,
    /// because hiding a window inside the event that says it lost focus is
    /// asking Windows to change the focus while it is telling you about the
    /// focus. The turn after is the earliest moment nothing is mid-transition,
    /// and it is the same turn the reader's own press would have been answered
    /// on.
    pending_dismiss: bool,
    /// **Whether the startup command has been run on this launch** (user ruling, 2026-09-05,
    /// §7.54e ⑤).
    ///
    /// The row's own word is "once": the command is the reader's answer to *what should be waiting
    /// in this window when I first reach for it today*, and a command that ran again on the fourth
    /// summon would be answering a different question. Held on the process rather than on the
    /// window because a reader who closes the summoned terminal and presses the key again has said
    /// "not this one" and not "run my startup command a second time" — which is
    /// [`Self::forget`]'s own ruling about the claim, said about the other thing that outlives the
    /// window.
    startup_command_spent: bool,
    /// **The rectangles the reader arranged this window at, one per display**
    /// (user ruling, next29 — see [`Self::placement_on`]).
    ///
    /// Read from the document at launch and written back to it on every
    /// snapshot, so it is the same list the file holds rather than a cache of
    /// one. Held on this struct and not on the window because the window is
    /// destroyed and reborn: a reader who closed the summoned terminal and
    /// pressed the key again gets the arrangement they made before they closed
    /// it, which is what a preference means.
    placements: Vec<bt_persist::QuakePlacementV1>,
}

impl Quake {
    /// The window, if this process has one.
    pub(crate) const fn window(&self) -> Option<WindowId> {
        self.window
    }

    /// Whether this id is the summoned window's.
    pub(crate) fn is_quake(&self, id: WindowId) -> bool {
        self.window == Some(id)
    }

    /// Whether the summoned window is on the screen.
    pub(crate) const fn is_showing(&self) -> bool {
        self.shown
    }

    /// **Whether another program holds the chord** — the settings page's one
    /// question.
    ///
    /// True only for the refusal that names a rival claimant. A chord this layout
    /// has no key for is a different fault with a different remedy (choose a key
    /// that exists), and it cannot arise from the recorder — which records keys
    /// that were actually pressed — so it is not a sentence the dialog carries.
    pub(crate) fn hotkey_taken(&self) -> bool {
        self.fault
            .as_ref()
            .is_some_and(HotkeyFault::is_already_registered)
    }

    /// Remember the window that has just been opened to be summoned.
    pub(crate) fn adopt(&mut self, id: WindowId) {
        self.window = Some(id);
        self.shown = false;
    }

    /// Forget it, because it has been closed.
    ///
    /// The claim is deliberately **kept**: the chord belongs to the process and
    /// not to the window, and a reader who closed the summoned terminal has said
    /// "not this one", not "stop answering the key". The next press opens a new
    /// one, which is the same sentence the first press said.
    pub(crate) fn forget(&mut self, id: WindowId) {
        if self.window == Some(id) {
            self.window = None;
            self.shown = false;
            self.give_back = None;
            self.pending_dismiss = false;
        }
    }

    /// A press has arrived.
    pub(crate) fn press(&mut self) {
        self.summoned = true;
    }

    /// Take the press, if there is one waiting.
    pub(crate) fn take_press(&mut self) -> bool {
        std::mem::take(&mut self.summoned)
    }

    /// The window lost the keyboard, and the row says that means goodbye.
    pub(crate) fn note_blur(&mut self) {
        self.pending_dismiss = true;
    }

    /// Take the blur, if one is waiting.
    pub(crate) fn take_dismiss(&mut self) -> bool {
        std::mem::take(&mut self.pending_dismiss)
    }

    /// **Whether a blur that has just been taken is a reason to put the window
    /// away** (§7.54c, user ruling 2026-09-03).
    ///
    /// Two facts and one rule, named here rather than spelled inline at the one
    /// place it is asked, because the rule is what the ruling moved and the
    /// window is the thing that knows one half of it.
    ///
    /// **The switch is read and not assumed.** Its default is now off —
    /// [`bt_persist::DEFAULT_QUAKE_DISMISS_ON_BLUR`], 「点到其他程序不是关闭
    /// 临时终端的途径」 — and the row it is on still means exactly what it says
    /// when a reader turns it on. Nothing about the mechanism changed with the
    /// default; a summon that has lost the keyboard while the row is on goes
    /// away, the way it always did.
    ///
    /// **A blur about a window that is already down is a blur about nothing**,
    /// which is why `shown` is half the answer: hiding the window is itself what
    /// moved the focus, and the blur that follows arrives after there is nothing
    /// left to dismiss.
    pub(crate) const fn blur_dismisses(&self, row_is_on: bool) -> bool {
        self.shown && row_is_on
    }

    /// Record that the window is up, and who is owed the keyboard back.
    pub(crate) fn shown_over(&mut self, previous: Option<NonZeroIsize>) {
        self.shown = true;
        self.pending_dismiss = false;
        self.give_back = previous;
    }

    /// Record that it is down, and hand back whoever was owed the keyboard.
    pub(crate) fn hidden(&mut self) -> Option<NonZeroIsize> {
        self.shown = false;
        self.pending_dismiss = false;
        self.give_back.take()
    }

    /// **The rectangle to summon onto this display**, or `None` when the reader
    /// has never arranged one there.
    ///
    /// This is the one place §7.54's "its rectangle is computed and never
    /// remembered" is narrowed, and it is narrowed rather than reversed. The
    /// objection that ruling was built on stands untouched: a corner read out of
    /// a file would put the window on the screen the reader was working on
    /// *yesterday*. What answers it is the key — an arrangement filed under the
    /// display it was made on can only ever come back on that display, and the
    /// first summon onto a display the reader has never arranged the window on
    /// computes the default shape exactly as it always did.
    ///
    /// **Nothing is remembered without a name for the display.** A machine whose
    /// monitor Windows will not name gets the computed rectangle every time,
    /// which is what every build before this one did on every machine.
    ///
    /// The stored rectangle is logical, so it is restated at the dpi of the
    /// display it is being summoned onto: the same window at the same size to the
    /// eye, on a display whose scale has changed since the reader sized it.
    #[must_use]
    pub(crate) fn placement_on(&self, monitor_id: Option<&str>, dpi: u32) -> Option<WindowRect> {
        let monitor_id = monitor_id?;
        let placement = self
            .placements
            .iter()
            .find(|placement| placement.monitor_id == monitor_id)?;
        Some(physical_rect(placement.bounds, dpi))
    }

    /// **Record that the reader put the window here, with their own hand.**
    ///
    /// One row per display, replaced rather than appended: the question a row
    /// answers is "where does this reader want it on this screen", and that
    /// question has one current answer.
    pub(crate) fn remember(&mut self, monitor_id: String, bounds: bt_persist::WindowBoundsV1) {
        if let Some(placement) = self
            .placements
            .iter_mut()
            .find(|placement| placement.monitor_id == monitor_id)
        {
            placement.bounds = bounds;
            return;
        }
        self.placements
            .push(bt_persist::QuakePlacementV1 { monitor_id, bounds });
    }

    /// What the document should hold, for the snapshot that writes it.
    #[must_use]
    pub(crate) fn placements(&self) -> Vec<bt_persist::QuakePlacementV1> {
        self.placements.clone()
    }

    /// What the document held, for the launch that reads it.
    pub(crate) fn adopt_placements(&mut self, placements: Vec<bt_persist::QuakePlacementV1>) {
        self.placements = placements;
    }

    /// **Where this summon puts the window** — the one door, and every entry calls it.
    ///
    /// There is exactly one gesture behind this window — the chord, whether it arrived from
    /// Windows' own claim or from the shortcut table when that claim was refused — and after the
    /// 2026-09-05 ruling exactly one function decides where the window lands when it does. That
    /// ruling is stated as a *shape* rather than as a habit: the machine is read in one place
    /// ([`SummonScreen::under_the_pointer`]) and the rules are applied in this one, so a second
    /// entry point cannot arrive with its own idea of the rectangle. `bt_app::main` never names
    /// [`summoned_rect`] or [`Self::placement_on`] at all, which is pinned by the source gate
    /// `a_summon_is_placed_by_one_function_and_main_does_not_do_the_geometry`.
    ///
    /// Two rules, in this order, and the order is the whole of it:
    ///
    /// * **A rectangle the reader arranged on this display wins**, restated at the scale that
    ///   display has now — see [`Self::placement_on`], which carries why remembering is narrow
    ///   enough to be safe.
    /// * **Otherwise it is computed**, which is what every summon did before next29 and what a
    ///   display nobody has arranged the window on still does — see [`summoned_rect`].
    #[must_use]
    pub(crate) fn placement(
        &self,
        screen: &SummonScreen,
        settings: &bt_persist::SettingsV1,
    ) -> WindowRect {
        self.placement_on(screen.monitor_id.as_deref(), screen.dpi)
            .unwrap_or_else(|| {
                summoned_rect(
                    screen.work,
                    settings.quake_width,
                    settings.quake_height,
                    settings.quake_top_gap,
                    screen.dpi,
                )
            })
    }

    /// **Take the one run of the startup command, if it has not been taken** (§7.54e ⑤).
    ///
    /// Named as a take rather than as a question with a setter beside it, for
    /// [`Self::take_press`]'s reason: a bit that is read in one place and cleared in another is a
    /// bit two turns can both believe they have, and this one runs a command.
    ///
    /// The empty row is not a command, and answering `None` for it here rather than at the call
    /// site is what keeps "did this run" and "was there anything to run" from being two facts that
    /// can disagree: a launch whose row was empty has spent nothing, so a reader who writes a
    /// command into the row mid-run gets it on their next summon.
    pub(crate) fn take_startup_command(&mut self, row: &str) -> Option<String> {
        if self.startup_command_spent || row.trim().is_empty() {
            return None;
        }
        self.startup_command_spent = true;
        Some(row.to_owned())
    }

    /// **Make the claim on the chord agree with the table**, and say nothing when
    /// it already does.
    ///
    /// Called once a turn, which is the only arrangement that keeps one statement
    /// true of a chord that can change from three doors — the recorder, a
    /// hand-edited `keybindings.json`, and `Restore all defaults`. Comparing
    /// against [`Self::claimed_for`] is what makes that free: the ordinary turn
    /// finds the chord it already asked for and does nothing at all.
    ///
    /// **A refusal is remembered rather than retried.** Windows answers
    /// `ERROR_HOTKEY_ALREADY_REGISTERED` to a second copy of this program for as
    /// long as the first one is running; asking again every turn would be a
    /// syscall sixty times a second to be told the same thing.
    pub(crate) fn reconcile(&mut self, wanted: Option<&Chord>) {
        if self.claimed_for.as_ref() == wanted {
            return;
        }
        // Dropped before the next is asked for, and that ordering is the whole of
        // it: a reader moving the summon between two chords that Windows
        // considers the same key would otherwise be refused their own claim.
        self.claim = None;
        self.claimed_for = wanted.cloned();
        self.fault = None;
        let Some(chord) = wanted else {
            return;
        };
        let Some(hotkey) = hotkey_for(chord) else {
            self.fault = Some(HotkeyFault::NoSuchKey);
            return;
        };
        match bt_platform::hotkey::register(SUMMON_HOTKEY_ID, hotkey) {
            Ok(claim) => self.claim = Some(claim),
            Err(fault) => {
                // **Silent to the reader** (ruling: a second instance's refusal is
                // expected, not an error). The settings page says so on the row
                // the window is described by; a card raised over somebody's editor
                // because a second copy of this program started would be a
                // notification about a thing that is working as designed.
                self.fault = Some(fault);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Quake, SummonScreen, hotkey_for, physical_rect, summoned_rect, typed_into_a_prompt,
    };
    use bt_platform::WindowRect;
    use bt_platform::hotkey::HotkeyFault;

    const fn work(left: i32, top: i32, right: i32, bottom: i32) -> WindowRect {
        WindowRect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// RED (§7.54) — **the summon takes its share of the top of the work area,
    /// centred across it** (user ruling, 2026-09-02).
    ///
    /// MUTATION: measure either percentage against the *monitor* instead of the
    /// work area and a window on a display with a top-docked taskbar opens
    /// underneath it. Drop the centring — leave `left` at `work.left`, which is
    /// what this function did until v28 — and the last assertion goes red naming
    /// two margins that are not the same number: the window hangs off the left
    /// edge of a 4K desk with everything it does not cover piled on the right.
    #[test]
    fn a_summon_takes_its_share_of_the_top_of_the_work_area_centred_across_it() {
        let area = work(0, 40, 1920, 1080);
        let rect = summoned_rect(area, 60, 40, 12, 96);
        assert_eq!(
            rect.top,
            40 + 12,
            "the work area's top, with the gap hung below it"
        );
        assert_eq!(
            rect.bottom,
            40 + 12 + 416,
            "forty percent of the 1040 rows the taskbar left, moved down"
        );
        assert_eq!(
            rect.right - rect.left,
            1152,
            "sixty percent of the 1920 columns"
        );
        assert_eq!(
            rect.left - area.left,
            area.right - rect.right,
            "and what it does not cover is the same on both sides of it"
        );
    }

    /// RED (§7.54) — **a hundred percent is the shape this window shipped as**,
    /// exactly and with nothing left over.
    ///
    /// The old behaviour is still reachable, and it is reachable through the top
    /// of the row's range rather than through a special case: a reader who really
    /// did want the full span has a value that says so. That is also what made the
    /// ruling above safe to make.
    ///
    /// MUTATION: round the width the other way, or centre a full-width window by
    /// halving a remainder that is not zero, and `100` stops meaning "the whole of
    /// it" — the window comes down a column short of the edge it used to reach.
    #[test]
    fn a_hundred_percent_wide_is_the_full_span_this_window_used_to_be() {
        let rect = summoned_rect(work(0, 40, 1920, 1080), 100, 40, 12, 96);
        assert_eq!(rect.left, 0, "it starts where the work area starts");
        assert_eq!(rect.right, 1920, "and ends where it ends");
    }

    /// RED (§7.54) — **a second monitor's rectangle is that monitor's.**
    ///
    /// MUTATION: use the virtual screen instead of the monitor the pointer is on
    /// and the window opens spanning every display at once. The negative origin
    /// is the case that catches it: a screen to the left of the primary one has a
    /// `left` below zero, and a rectangle computed from a width would land on the
    /// wrong display. The centred half is caught here too — sixty percent of this
    /// screen is centred on *this* screen, and a centring computed from zero
    /// rather than from `work.left` would put the window on the primary one.
    #[test]
    fn a_screen_left_of_the_primary_one_keeps_its_own_negative_origin() {
        let full = summoned_rect(work(-1920, 0, 0, 1200), 100, 50, 12, 96);
        assert_eq!(full.left, -1920);
        assert_eq!(full.right, 0);
        assert_eq!(full.top, 12);
        assert_eq!(full.bottom, 12 + 600);
        let centred = summoned_rect(work(-1920, 0, 0, 1200), 60, 50, 12, 96);
        assert_eq!(
            centred.left, -1536,
            "384 columns in from this screen's edge"
        );
        assert_eq!(centred.right, -384, "and 384 short of its other one");
    }

    /// RED (§7.54) — **a hand-edited size is clamped where the window is
    /// placed, not where the file is read.**
    ///
    /// MUTATION: drop the height clamp and `"quake_height": 400` opens a window
    /// four screens tall whose tab strip is the only part on the display; `0`
    /// opens one with no room for a shell at all. Drop the *width* clamp and
    /// `"quake_width": 5` opens a column too narrow for a prompt — and `250`
    /// opens one two and a half screens wide whose centring pushes its left edge
    /// off the display, which is a window a reader cannot even find to close.
    /// `bt_persist` stores what it was given by design, so this is the surface
    /// that owes both ranges.
    #[test]
    fn a_size_no_row_offers_is_clamped_into_the_one_that_is() {
        let tall = summoned_rect(work(0, 0, 1000, 1000), 60, 250, 12, 96);
        assert_eq!(
            tall.bottom - tall.top,
            1000,
            "nothing taller than the work area"
        );
        let flat = summoned_rect(work(0, 0, 1000, 1000), 60, 0, 12, 96);
        assert_eq!(
            flat.bottom - flat.top,
            i32::from(bt_persist::MINIMUM_QUAKE_HEIGHT) * 10,
            "and nothing shorter than the row's own floor"
        );
        let broad = summoned_rect(work(0, 0, 1000, 1000), 250, 40, 12, 96);
        assert_eq!(broad.left, 0, "nothing wider than the work area");
        assert_eq!(broad.right, 1000);
        let narrow = summoned_rect(work(0, 0, 1000, 1000), 5, 40, 12, 96);
        assert_eq!(
            narrow.right - narrow.left,
            i32::from(bt_persist::MINIMUM_QUAKE_WIDTH) * 10,
            "and nothing narrower than the width row's own floor"
        );
        assert_eq!(
            narrow.left,
            (1000 - i32::from(bt_persist::MINIMUM_QUAKE_WIDTH) * 10) / 2,
            "clamped and then centred, in that order — a floor applied after the \
             centring would pin the narrowest window to the left edge"
        );
    }

    /// RED (§7.54) — **an odd remainder stays inside the work area**, and the
    /// leftover column goes to the right.
    ///
    /// A centred window inside an odd number of uncovered columns cannot have two
    /// equal margins, and the guarantee that survives is the one that matters:
    /// neither edge leaves the screen. 3441 is a real ultrawide's width plus the
    /// pixel that makes the arithmetic odd — the case that is wrong on a machine
    /// and right in a spreadsheet.
    ///
    /// MUTATION: round the left margin *up* — `(wide - width).div_ceil(2)`, or
    /// `(wide - width + 1) / 2` — and `right` lands one column past `work.right`,
    /// which on a multi-monitor desk is one column on the neighbouring display.
    #[test]
    fn an_odd_remainder_puts_the_leftover_column_inside_the_work_area() {
        let rect = summoned_rect(work(0, 0, 3441, 1440), 60, 40, 12, 96);
        assert_eq!(rect.right - rect.left, 2064, "sixty percent of 3441");
        assert!(rect.left >= 0, "the left edge is on the screen");
        assert!(
            rect.right <= 3441,
            "and so is the right one: {} is past the work area",
            rect.right
        );
        assert_eq!(
            rect.left, 688,
            "the odd column is on the right, where the margins are 688 and 689"
        );
    }

    /// RED (§7.54) — **every modifier a chord wears reaches the hotkey, and the
    /// key half is asked of the layout.**
    ///
    /// The app's half of the translation `bt_platform::hotkey::registration_bits`
    /// finishes: four booleans out of one `ModifiersState`, and a virtual key out
    /// of the same reader a page's accelerator uses. A digit is the fixture
    /// because its virtual key is the digit's own ASCII on every Latin layout, so
    /// the assertion is about the crossing rather than about this machine.
    ///
    /// MUTATION: read `SUPER` into `alt`, or drop it, and a summon bound with the
    /// Windows key registers as something else — a chord the reader never bound,
    /// claimed process-wide, and taken out of every other program's input stream.
    #[test]
    fn every_modifier_a_chord_wears_reaches_the_hotkey() {
        use crate::shortcuts::{Chord, ChordKey};
        use std::borrow::Cow;
        use winit::keyboard::ModifiersState;

        let of = |modifiers: ModifiersState| {
            hotkey_for(&Chord {
                modifiers,
                key: ChordKey::Character(Cow::Borrowed("1")),
            })
            .expect("a digit is a key every Latin layout can produce")
        };
        let bare = of(ModifiersState::empty());
        assert_eq!(bare.virtual_key, 0x31, "the layout answers `1` with VK_1");
        assert!(!bare.ctrl && !bare.alt && !bare.shift && !bare.win);
        assert!(of(ModifiersState::CONTROL).ctrl);
        assert!(of(ModifiersState::ALT).alt);
        assert!(of(ModifiersState::SHIFT).shift);
        assert!(of(ModifiersState::SUPER).win);
        let all = of(ModifiersState::CONTROL
            .union(ModifiersState::ALT)
            .union(ModifiersState::SHIFT)
            .union(ModifiersState::SUPER));
        assert!(
            all.ctrl && all.alt && all.shift && all.win,
            "and all four together"
        );
    }

    /// RED (§7.54) — **ten summons and ten dismissals leave the state exactly
    /// where one did.**
    ///
    /// The show and the hide run on every press, and the three things showing a
    /// window does were written for a door that runs once. This holds the half of
    /// that claim a test can hold without a window: the bits that decide which way
    /// the next press goes, over the run of cycles the real machine is measured
    /// on.
    ///
    /// MUTATION: leave `shown` set in `hidden`, or clear it in `shown_over`, and
    /// the toggle inverts after the first cycle — the key that put the window up
    /// puts it up again, and the one that sent it away sends it away twice.
    #[test]
    fn ten_cycles_leave_the_summon_exactly_where_one_did() {
        use std::num::NonZeroIsize;

        let mut quake = Quake::default();
        for cycle in 1..=10_isize {
            let over = NonZeroIsize::new(0x1000 + cycle);
            assert!(!quake.is_showing(), "cycle {cycle} starts with it away");
            quake.shown_over(over);
            assert!(quake.is_showing(), "cycle {cycle} put it up");
            assert!(
                !quake.take_dismiss(),
                "cycle {cycle}: putting it up is not a reason to send it away"
            );
            quake.note_blur();
            assert!(quake.take_dismiss(), "cycle {cycle} noticed the blur");
            assert_eq!(
                quake.hidden(),
                over,
                "cycle {cycle} hands back the window it came down over, and not \
                 the one an earlier cycle did"
            );
        }
        assert!(!quake.is_showing());
        assert_eq!(quake.hidden(), None, "and nothing is owed at the end of it");
    }

    /// RED (§7.54) — **the settings page speaks only for the refusal a reader can
    /// do something about.**
    ///
    /// MUTATION: report every fault as "another program has this key" and a chord
    /// this layout cannot produce comes back accusing an innocent program.
    #[test]
    fn only_a_rival_claim_is_a_sentence_the_dialog_carries() {
        let mut quake = Quake::default();
        assert!(!quake.hotkey_taken(), "a claim nobody made cannot be taken");
        quake.fault = Some(HotkeyFault::AlreadyRegistered);
        assert!(quake.hotkey_taken());
        quake.fault = Some(HotkeyFault::NoSuchKey);
        assert!(!quake.hotkey_taken());
        quake.fault = Some(HotkeyFault::Refused("something else entirely".to_owned()));
        assert!(!quake.hotkey_taken());
    }

    /// RED (§7.54) — **a chord that has not moved is not re-asked for.**
    ///
    /// The decision in front of the registration, which is the part that runs
    /// sixty times a second. The chord is deliberately one **no layout on any
    /// machine can produce** — a character outside the basic plane, which
    /// `virtual_key_for_character` has no single UTF-16 unit to ask about — so
    /// this never reaches Windows and answers the same on every host. That is
    /// also what makes the assertion sharp: a `reconcile` that acted would leave
    /// [`HotkeyFault::NoSuchKey`] behind, and one that did not leaves the refusal
    /// it was handed.
    ///
    /// MUTATION: drop the equality check at the top of `reconcile` and every turn
    /// of the loop unregisters and re-registers the summon — which on a second
    /// instance means one refused syscall per frame, for ever. The first
    /// assertion then goes red naming the fault the re-decision produced.
    #[test]
    fn a_chord_that_has_not_moved_is_left_alone() {
        use crate::shortcuts::{Chord, ChordKey};
        use std::borrow::Cow;
        use winit::keyboard::ModifiersState;

        let chord = Chord {
            modifiers: ModifiersState::CONTROL,
            key: ChordKey::Character(Cow::Borrowed("\u{1f5dd}")),
        };
        assert!(
            hotkey_for(&chord).is_none(),
            "the fixture is a chord no keyboard can produce, so nothing here \
             depends on the machine it runs on"
        );
        let mut quake = Quake {
            claimed_for: Some(chord.clone()),
            fault: Some(HotkeyFault::AlreadyRegistered),
            ..Quake::default()
        };
        quake.reconcile(Some(&chord));
        assert_eq!(
            quake.fault,
            Some(HotkeyFault::AlreadyRegistered),
            "the refusal is remembered rather than re-decided; a `reconcile` that \
             had acted would have left `NoSuchKey` here instead"
        );
        quake.reconcile(None);
        assert!(
            quake.fault.is_none() && quake.claimed_for.is_none(),
            "and a chord taken away releases the claim and the fault with it"
        );
    }

    /// RED (§7.54) — **the window a summon shows over is remembered once and
    /// spent once.**
    ///
    /// MUTATION: leave `give_back` in place after a dismissal and the second
    /// dismissal hands the keyboard to whatever program happened to be in front
    /// before the *first* summon — very often one that has since been closed.
    #[test]
    fn the_window_owed_the_keyboard_is_handed_back_exactly_once() {
        use std::num::NonZeroIsize;

        let previous = NonZeroIsize::new(0x1234);
        let mut quake = Quake::default();
        quake.shown_over(previous);
        assert!(quake.is_showing());
        assert_eq!(quake.hidden(), previous);
        assert!(!quake.is_showing());
        assert_eq!(
            quake.hidden(),
            None,
            "there is nobody left to hand it to the second time"
        );
    }

    /// RED (§7.54) — **a blur is a bit and a turn, never an act inside the
    /// event.**
    ///
    /// MUTATION: have the blur hide the window where it is noticed and the
    /// program is changing the focus while Windows is in the middle of telling it
    /// about the focus. The two-step is also what lets a summon that is showing
    /// be dismissed by its own key: the press and the blur it causes land in one
    /// turn and mean one thing.
    /// RED (§7.54, user ruling next29) — **the summoned window hangs a gap
    /// below the top of the work area, and the gap is logical pixels.**
    ///
    /// The window is drawn with rounded corners; flush against the top of a
    /// display, the two upper corners show square notches where the rounding
    /// stops. Twelve logical pixels of desktop above it is what makes it read as
    /// a panel that came down over the screen rather than one the screen cut off.
    ///
    /// **The height is moved, not cropped.** The reader's percentage answers "how
    /// much of this screen", and a percentage that quietly returned twelve pixels
    /// less than it said would be a different number at every scale.
    ///
    /// MUTATION: leave `top` at `work.top` and the gap is gone — the first
    /// assertion goes red naming the work area's own top. Scale the gap at the
    /// window's cached dpi instead of the summoned display's, or not at all, and
    /// the third assertion goes red: on a 200% display the gap is either half the
    /// size it should be or a different size on every press, depending on which
    /// display the window was last summoned onto. Pay for the gap out of the
    /// height — `bottom` unchanged — and the second goes red.
    #[test]
    fn the_summon_hangs_a_scaled_gap_below_the_top_of_the_work_area() {
        let area = work(0, 100, 1000, 1100);
        let at_96 = summoned_rect(area, 60, 50, 12, 96);
        assert_eq!(
            at_96.top, 112,
            "twelve logical pixels below the work area's own top"
        );
        assert_eq!(
            at_96.bottom - at_96.top,
            500,
            "the height asked for is untouched - moved down, not cut short"
        );
        let at_192 = summoned_rect(area, 60, 50, 12, 192);
        assert_eq!(
            at_192.top, 124,
            "the same gap on a 200% display is twice the physical pixels"
        );
        assert_eq!(
            at_192.left, at_96.left,
            "a fraction of the width answers the same at every scale"
        );
    }

    /// RED (§7.54, user ruling next29) — **a rectangle the reader arranged comes
    /// back on the display they arranged it on, and nowhere else.**
    ///
    /// The narrowing of "computed and never remembered", and the whole of what
    /// makes it safe: the objection to reading a rectangle out of a file was that
    /// it would open the window on yesterday's screen, and an answer filed under
    /// a display cannot be given for a different one.
    ///
    /// MUTATION: key the placements on nothing — return the first one whatever
    /// display is asked for — and the second assertion goes red: the arrangement
    /// made on the wide screen comes back on the laptop panel. Drop the `?` on
    /// the monitor name and a machine whose display Windows will not name starts
    /// serving another display's rectangle; the third assertion catches it. Keep
    /// appending instead of replacing and the fourth goes red — the reader's
    /// second arrangement is filed behind their first and never read again.
    #[test]
    fn an_arranged_rectangle_comes_back_only_on_the_display_it_was_arranged_on() {
        // The shape Windows answers `monitor_id_at` with, spelled once.
        const FIRST: &str = r"\\.\DISPLAY1";
        const SECOND: &str = r"\\.\DISPLAY2";
        let bounds = |x, y, width, height| bt_persist::WindowBoundsV1 {
            x,
            y,
            width,
            height,
        };
        let mut quake = Quake::default();
        assert_eq!(
            quake.placement_on(Some(FIRST), 96),
            None,
            "a display nobody has arranged the window on computes its shape"
        );
        quake.remember(FIRST.to_owned(), bounds(100, 50, 800, 400));
        assert_eq!(
            quake.placement_on(Some(FIRST), 96),
            Some(work(100, 50, 900, 450)),
            "the display it was arranged on gives it back"
        );
        assert_eq!(
            quake.placement_on(Some(SECOND), 96),
            None,
            "the other display has never been arranged and keeps the default"
        );
        assert_eq!(
            quake.placement_on(None, 96),
            None,
            "a display Windows will not name remembers nothing at all"
        );
        quake.remember(FIRST.to_owned(), bounds(0, 0, 640, 480));
        assert_eq!(
            quake.placements().len(),
            1,
            "one display, one answer - the second replaces the first"
        );
        assert_eq!(
            quake.placement_on(Some(FIRST), 96),
            Some(work(0, 0, 640, 480)),
            "and it is the second one that comes back"
        );
    }

    /// RED (§7.54, user ruling next29) — **an arrangement keeps its size to the
    /// eye when the display's scale changes.**
    ///
    /// The reason the file stores logical pixels: what the reader chose is a
    /// window of a certain size *on a screen*, not a certain number of the
    /// pixels that screen happened to have that day.
    ///
    /// MUTATION: hand the stored numbers to `stand_window_at` as physical pixels
    /// and a reader who raises their display's scaling finds the terminal they
    /// arranged has shrunk to half of it.
    #[test]
    fn an_arrangement_is_restated_at_the_scale_the_display_now_has() {
        let bounds = bt_persist::WindowBoundsV1 {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        };
        assert_eq!(physical_rect(bounds, 96), work(10, 20, 310, 220));
        assert_eq!(physical_rect(bounds, 192), work(20, 40, 620, 440));
        let hairline = bt_persist::WindowBoundsV1 {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        let floored = physical_rect(hairline, 96);
        assert_eq!(
            (floored.right - floored.left, floored.bottom - floored.top),
            (1, 1),
            "a rectangle of no size is floored where the window is placed"
        );
    }

    #[test]
    fn a_blur_is_taken_once_and_showing_the_window_clears_it() {
        let mut quake = Quake::default();
        assert!(!quake.take_dismiss());
        quake.note_blur();
        assert!(quake.take_dismiss());
        assert!(!quake.take_dismiss(), "and only once");
        quake.note_blur();
        quake.shown_over(None);
        assert!(
            !quake.take_dismiss(),
            "a window that has just been put up is not a window that was just \
             left"
        );
    }

    /// RED (§7.54c, user ruling 2026-09-03) — **the default moved and the
    /// mechanism did not: a reader who turns the row on still loses the summon
    /// the moment the keyboard leaves it.**
    ///
    /// The ruling is 「点到其他程序不是关闭临时终端的途径」, and it is a ruling
    /// about what a fresh `settings.json` says
    /// ([`bt_persist::DEFAULT_QUAKE_DISMISS_ON_BLUR`]) rather than about what the
    /// row does. The row is why the row exists: somebody who wants a strip that
    /// empties the moment they look away asks for it there and gets exactly that.
    /// A change that quietly took the behaviour away along with the default would
    /// leave a switch on the General page that decides nothing.
    ///
    /// MUTATIONS: drop `row_is_on` from the conjunction and a click into a
    /// browser takes the terminal away from every reader again, with the row they
    /// left off saying it should not. Drop `self.shown` and a blur that arrived
    /// about a window already down asks for a second dismissal, which is the
    /// double hand-back `the_window_owed_the_keyboard_is_handed_back_exactly_once`
    /// pins. Return `false` outright and the switch is a row on a page that
    /// decides nothing.
    #[test]
    fn the_row_still_means_what_it_says_when_a_reader_turns_it_on() {
        let mut quake = Quake::default();
        // Down: nothing to dismiss, whatever the row says — hiding the window is
        // itself what moved the focus.
        assert!(!quake.blur_dismisses(true));
        assert!(!quake.blur_dismisses(false));
        quake.shown_over(None);
        assert!(
            quake.blur_dismisses(true),
            "the reader asked for a terminal that goes away when they look \
             elsewhere, and did not get one"
        );
        assert!(
            !quake.blur_dismisses(false),
            "a click into another program is being read as a request to close \
             the summoned terminal"
        );
        // And the default the ruling set is what a fresh file hands in here,
        // which is the whole of what this round changed.
        assert!(!quake.blur_dismisses(bt_persist::DEFAULT_QUAKE_DISMISS_ON_BLUR));
    }

    /// RED (§7.54e, user ruling 2026-09-05) — **one function decides where a summon lands, and
    /// `main` does not do the geometry.**
    ///
    /// The ruling is 「呼出规则唯一…写成一个函数,所有入口调它」, and a test that only exercised
    /// [`Quake::placement`] would pass against a build with a second, quieter rectangle computed at
    /// some other door. So half of this is a source gate: the two functions that *are* the geometry
    /// are named nowhere but this file, and the one place `main` may say is `placement`.
    ///
    /// MUTATION: compute a rectangle at any call site in `main` — inline `summoned_rect`, or ask
    /// `placement_on` and fall back by hand, which is exactly what `show_quake_window` used to do —
    /// and the first assertion names the file it was written in. Take the remembered rectangle out
    /// of `placement` and the third goes red: a reader's own arrangement stops coming back. Ask
    /// `placement_on` with the *window's* dpi rather than the summoned display's and the fourth
    /// goes red, which is the same seam `an_arrangement_is_restated_at_the_scale_the_display_now_has`
    /// pins one layer down.
    #[test]
    fn a_summon_is_placed_by_one_function_and_main_does_not_do_the_geometry() {
        const MAIN: &str = include_str!("main.rs");
        for name in ["summoned_rect", "placement_on"] {
            assert!(
                !MAIN.contains(name),
                "`{name}` is the summon's geometry and it is being done outside \
                 `quake::Quake::placement`, which is the one door the ruling asks for"
            );
        }
        assert!(
            MAIN.contains("self.app.quake.placement(&screen, settings)"),
            "the one door is not being called from the one place that shows the window"
        );

        let settings = bt_persist::SettingsV1::default();
        let screen = SummonScreen {
            work: work(0, 0, 1920, 1080),
            monitor_id: Some(r"\\.\DISPLAY1".to_owned()),
            dpi: 96,
        };
        let mut quake = Quake::default();
        assert_eq!(
            quake.placement(&screen, &settings),
            summoned_rect(
                screen.work,
                settings.quake_width,
                settings.quake_height,
                settings.quake_top_gap,
                screen.dpi,
            ),
            "a display nobody has arranged the window on computes its shape, as it always did"
        );
        quake.remember(
            r"\\.\DISPLAY1".to_owned(),
            bt_persist::WindowBoundsV1 {
                x: 30,
                y: 40,
                width: 500,
                height: 300,
            },
        );
        assert_eq!(
            quake.placement(&screen, &settings),
            work(30, 40, 530, 340),
            "and a rectangle the reader arranged with their own hand wins over the computed one"
        );
        let doubled = SummonScreen {
            dpi: 192,
            ..screen.clone()
        };
        assert_eq!(
            quake.placement(&doubled, &settings),
            work(60, 80, 1060, 680),
            "restated at the scale the display has now, so it is the same size to the eye"
        );
    }

    /// RED (§7.54e ⑤, user ruling 2026-09-05) — **the startup command runs once per launch, and an
    /// empty row is not a command.**
    ///
    /// The one string this product executes on the reader's behalf, and the whole of what makes
    /// that acceptable is that they wrote it into a row whose sentence says it will be run — once.
    ///
    /// MUTATION: leave the bit unset and every summon re-runs it, so a reader whose row says
    /// `git pull` finds one running every time they reach for the terminal. Set the bit for an
    /// empty row and a reader who fills the row in mid-run never gets their command at all. Read
    /// the bit somewhere and clear it somewhere else and two turns can both believe they hold it,
    /// which is `take_press`'s own ruling about a bit that is spent.
    #[test]
    fn a_startup_command_is_taken_once_a_launch_and_an_empty_row_is_not_one() {
        let mut quake = Quake::default();
        assert_eq!(quake.take_startup_command(""), None, "no row, no command");
        assert_eq!(
            quake.take_startup_command("   "),
            None,
            "and whitespace is not a command either"
        );
        assert_eq!(
            quake.take_startup_command("fastfetch"),
            Some("fastfetch".to_owned()),
            "the first summon of a launch runs what the row says"
        );
        assert_eq!(
            quake.take_startup_command("fastfetch"),
            None,
            "and no summon after it does"
        );
        assert_eq!(
            quake.take_startup_command("git pull"),
            None,
            "including one made after the row was edited: once is once"
        );
    }

    /// RED (§7.54e ④, user ruling 2026-09-05) — **a restored command is typed at the prompt and
    /// never submitted**, and a submitted one is submitted with exactly one byte.
    ///
    /// 「绝不自动执行任何命令」 is the ruling, and the assertion that carries it is the one about
    /// bytes: what reaches the pty for a restored command contains no `\r` and no `\n`, so there
    /// is nothing in it a shell can read as the end of a line.
    ///
    /// **The hand-edited document is the case that matters.** A `session.json` somebody typed a
    /// `\r` into would, without the filter, restore a command that ran itself — the ruling would
    /// then hold only for documents this build wrote, which is not a ruling at all.
    ///
    /// MUTATION: drop the control-character filter and the third assertion goes red carrying a
    /// carriage return into a shell. Push `\n` instead of `\r` for the submitted half and a
    /// ConPTY-hosted shell is handed a byte it does not read as a submission. Return the bytes for
    /// an empty command and the first summon of a run sends a bare `Enter` into a fresh prompt.
    #[test]
    fn a_restored_command_is_typed_at_the_prompt_and_never_submitted() {
        assert_eq!(
            typed_into_a_prompt("cargo build", false),
            b"cargo build".to_vec(),
            "a restored command arrives standing at the prompt, with nothing to end its line"
        );
        assert_eq!(
            typed_into_a_prompt("cargo build", true),
            b"cargo build\r".to_vec(),
            "and the one command the reader asked to have run is ended with the byte a keyboard \
             sends"
        );
        let hand_edited = typed_into_a_prompt("cargo build\r\nrm -rf /", false);
        assert!(
            !hand_edited.contains(&b'\r') && !hand_edited.contains(&b'\n'),
            "a document with a newline in it restored a command that ran itself: {hand_edited:?}"
        );
        assert_eq!(
            hand_edited,
            b"cargo buildrm -rf /".to_vec(),
            "what is left is text, and text at a prompt is a thing the reader can read and edit"
        );
        assert!(
            typed_into_a_prompt("", true).is_empty(),
            "an empty row is not a command, and a bare Enter into a fresh prompt is not nothing"
        );
        assert!(
            typed_into_a_prompt("\r\n\t", true).is_empty(),
            "and neither is a field with nothing but control bytes in it"
        );
    }

    /// RED (§7.54e ⑤, user ruling 2026-09-05) — **the gap under the top of the screen is the
    /// reader's number now**, clamped where the window is placed.
    ///
    /// next29 wired twelve logical pixels into this file with a real argument behind it
    /// (§7.54b ④); the row does not overturn the argument, it gives the reader who wants the panel
    /// flush against the edge — or further down it — a way to say so.
    ///
    /// MUTATION: go on reading a constant and the first two assertions collapse onto each other,
    /// so a row that decides nothing ships. Drop the ceiling and a hand-edited `1000` opens the
    /// window below the bottom of the screen. Put a floor under it and zero — the shape this
    /// window had before the gap existed — becomes unreachable.
    #[test]
    fn the_gap_over_the_summon_is_the_row_and_is_clamped_where_the_window_is_placed() {
        let area = work(0, 100, 1000, 1100);
        assert_eq!(
            summoned_rect(area, 60, 50, 0, 96).top,
            100,
            "flush, if that is what the row says"
        );
        assert_eq!(
            summoned_rect(area, 60, 50, 40, 96).top,
            140,
            "and forty if it says forty"
        );
        assert_eq!(
            summoned_rect(area, 60, 50, 4_000, 96).top,
            100 + i32::try_from(bt_persist::MAXIMUM_QUAKE_TOP_GAP).unwrap(),
            "a hand-edited file cannot push the window off the bottom of the screen"
        );
        assert_eq!(
            summoned_rect(area, 60, 50, 40, 192).top,
            180,
            "and the row is logical pixels, so it is the same seam at every scale"
        );
    }
}
