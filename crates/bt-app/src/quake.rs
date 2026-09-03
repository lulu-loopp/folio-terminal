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
#[must_use]
pub(crate) fn summoned_rect(work: WindowRect, width_percent: u8, height_percent: u8) -> WindowRect {
    let vertical = height_percent.clamp(bt_persist::MINIMUM_QUAKE_HEIGHT, 100);
    let horizontal = width_percent.clamp(bt_persist::MINIMUM_QUAKE_WIDTH, 100);
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
    WindowRect {
        left,
        top: work.top,
        right: left.saturating_add(width),
        bottom: work.top.saturating_add(height),
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
    use super::{Quake, hotkey_for, summoned_rect};
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
        let rect = summoned_rect(area, 60, 40);
        assert_eq!(
            rect.top, 40,
            "the top is the work area's, not the monitor's"
        );
        assert_eq!(
            rect.bottom,
            40 + 416,
            "forty percent of the 1040 rows the taskbar left"
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
        let rect = summoned_rect(work(0, 40, 1920, 1080), 100, 40);
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
        let full = summoned_rect(work(-1920, 0, 0, 1200), 100, 50);
        assert_eq!(full.left, -1920);
        assert_eq!(full.right, 0);
        assert_eq!(full.top, 0);
        assert_eq!(full.bottom, 600);
        let centred = summoned_rect(work(-1920, 0, 0, 1200), 60, 50);
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
        let tall = summoned_rect(work(0, 0, 1000, 1000), 60, 250);
        assert_eq!(tall.bottom, 1000, "nothing taller than the work area");
        let flat = summoned_rect(work(0, 0, 1000, 1000), 60, 0);
        assert_eq!(
            flat.bottom,
            i32::from(bt_persist::MINIMUM_QUAKE_HEIGHT) * 10,
            "and nothing shorter than the row's own floor"
        );
        let broad = summoned_rect(work(0, 0, 1000, 1000), 250, 40);
        assert_eq!(broad.left, 0, "nothing wider than the work area");
        assert_eq!(broad.right, 1000);
        let narrow = summoned_rect(work(0, 0, 1000, 1000), 5, 40);
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
        let rect = summoned_rect(work(0, 0, 3441, 1440), 60, 40);
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
}
