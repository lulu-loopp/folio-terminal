//! The window's own tooltip: one host, any anchor.
//!
//! `title=` is the operating system's box, and the operating system's box obeys
//! nothing this window decides — not the theme, not the type, not the corner
//! radius, and it appears on its own schedule wherever the platform feels like
//! putting it (mock-up 1199-1206). So the tip is ours, drawn in the same
//! material as every other popup, and this module is the whole of it.
//!
//! Three pieces, deliberately apart:
//!
//! * [`TooltipAnchors`] — what is tippable *this frame*. Rebuilt from live
//!   geometry every time the chrome is, so a tip can never show a string the
//!   thing under the pointer stopped meaning.
//! * [`TooltipHost`] — the singleton state: which anchor is settling, which one
//!   is showing, and the two clocks that move between them. It knows nothing
//!   about tabs.
//! * [`layout`] and [`build`] — the box, placed and painted.
//!
//! The split is what lets a button that does not exist yet get a tooltip by
//! pushing one line into an anchor list.

use std::time::{Duration, Instant};

use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad};
use bt_term::ProgressState;

use crate::marks::OverlayLayer;
use crate::settings::push_float_window;
use crate::{EASE, Motion, cubic_bezier};

/// How long the pointer must rest on an anchor before its tip appears
/// (mock-up 8716).
///
/// This really is the 300-500ms case the guidance is about: a tip is content
/// laid *over* what you are reading, so a false positive costs you the view.
/// (The hover-peek flyout is the opposite case and gets its own, shorter clock.)
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(380);

/// `transition: opacity .09s ease` (mock-up 1220).
pub const TOOLTIP_FADE: Duration = Duration::from_millis(90);

/// `border-radius: 5px`.
pub const TIP_RADIUS_LOGICAL_PX: f32 = 5.0;
/// `border: 1px solid var(--border)`.
pub const TIP_BORDER_LOGICAL_PX: f32 = 1.0;
/// The `7px` of `padding: 3px 7px`.
pub const TIP_PADDING_X_LOGICAL_PX: f32 = 7.0;
/// The `3px` of `padding: 3px 7px`.
pub const TIP_PADDING_Y_LOGICAL_PX: f32 = 3.0;
/// `font-size: 11px`.
pub const TIP_FONT_LOGICAL_PX: f32 = 11.0;
/// The one number `showTip` uses for both jobs (mock-up 8698-8703): how far the
/// tip stands off its host, and how close it may come to the window's edge.
/// They are the same gap in the mock-up and stay one constant here, because the
/// day they differ is the day someone has to explain why.
pub const TIP_GAP_LOGICAL_PX: f32 = 6.0;

/// Which anchor a tip belongs to.
///
/// Identity only — never the text. The text is recomputed from the anchor every
/// frame (mock-up's own `el.title = tabTip(w)` on every paint), so a tip that is
/// already up follows its subject: rename the tab under an open tip and the tip
/// says the new name on the next frame.
///
/// Deliberately *not* [`crate::seats::ChromeTarget`]. That enum answers "what
/// does a click here do", and the two questions have different shapes: the tab's
/// mark is a tip of its own while a command runs (D38) but has never been a
/// click target, and the `×` is a click target the mock-up gives no tip at all.
/// Folding them together would have meant teaching the press, drag and cursor
/// machinery about a target that exists only to be hovered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TooltipAnchorId {
    /// The tab body — index into the strip, matching `ChromeTarget::Tab`.
    Tab(usize),
    /// The mark slot: the working icon, or the progress ring that replaces it.
    TabIcon(usize),
    /// The pin, in the `×`'s own slot.
    TabPin(usize),
    NewTab,
    NewTabMenu,
    Settings,
    /// `.panel-toggle` — the rail's fold-away button, which the vertical layout
    /// puts at the far left of the title bar.
    PanelToggle,
    Minimize,
    Maximize,
    CloseWindow,
}

/// One tippable box.
#[derive(Clone, Debug, PartialEq)]
pub struct TooltipAnchor {
    pub id: TooltipAnchorId,
    /// `[left, top, right, bottom]`, physical pixels of the whole surface.
    pub rect: [f32; 4],
    /// What the tip says. Never empty — see [`TooltipAnchors::push`].
    pub text: String,
}

/// Everything tippable this frame, innermost first.
///
/// The mock-up resolves an anchor with `target.closest("[title], [data-tip]")`
/// and takes the *nearest* one up the tree, so a control inside a tab answers
/// before the tab does. A flat list in innermost-first order is that rule
/// without a tree: push the children, then the thing they sit in, and the first
/// box that contains the pointer wins.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TooltipAnchors {
    entries: Vec<TooltipAnchor>,
}

impl TooltipAnchors {
    /// Register an anchor — unless it has nothing to say, in which case it is not
    /// an anchor at all (M141).
    ///
    /// "No text" and "empty text" are the same thing here, and that is the whole
    /// mechanism behind the mock-up's most careful line: `paintStrip` *removes*
    /// the icon's `title` when the command stops running rather than setting it
    /// to `""` (mock-up 4341-4343), because an empty tip would not merely draw an
    /// empty box — it would stop the pointer from ever reaching the tab
    /// underneath. Refusing to register is what lets the idle mark fall through
    /// to its tab.
    pub fn push(&mut self, id: TooltipAnchorId, rect: [f32; 4], text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() || rect[2] <= rect[0] || rect[3] <= rect[1] {
            return;
        }
        self.entries.push(TooltipAnchor { id, rect, text });
    }

    /// The innermost anchor under this point.
    #[must_use]
    pub fn at(&self, x: f32, y: f32) -> Option<&TooltipAnchor> {
        self.entries.iter().find(|anchor| {
            x >= anchor.rect[0] && x < anchor.rect[2] && y >= anchor.rect[1] && y < anchor.rect[3]
        })
    }

    /// This frame's box and text for an anchor that is already showing.
    ///
    /// `None` when the anchor is gone — the tab closed, the strip scrolled it
    /// away, the command finished and its ring with it. A tip whose subject has
    /// left has nothing to say and is taken down.
    #[must_use]
    pub fn find(&self, id: TooltipAnchorId) -> Option<&TooltipAnchor> {
        self.entries.iter().find(|anchor| anchor.id == id)
    }
}

/// Which layer of the title stack a tab's displayed name actually came from
/// (mock-up 3010: `nameSource`).
///
/// Where a name came from is real information, and the mock-up spends exactly
/// one tooltip on it (4193-4196): a badge on every tab would spend permanent
/// pixels on a question you ask twice a month.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameSource {
    /// You typed it.
    Manual,
    /// The program announced it (OSC 2).
    Program,
    /// It is the working folder's leaf (OSC 7).
    Cwd,
}

impl NameSource {
    /// The mock-up's own wording — `NAME_SOURCE`, line 3011. Not paraphrased:
    /// these three strings are the entire user-facing explanation of a system
    /// with four layers in it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Named by you",
            Self::Program => "Set by the program",
            Self::Cwd => "Working folder",
        }
    }
}

/// The one mark this window writes between a name and the place that name
/// belongs to.
///
/// Two readers, one glyph, and it is a constant rather than two literals because
/// they are deliberately the *same* punctuation saying the same thing: the tab
/// tip's `Working folder · C:\src` (M140) and the pane head's `vim main.rs ·
/// C:\src`, where a program has announced something and the head still has to
/// say where that something is standing. A head that separated its two halves
/// with a dash while the tip beside it used a middle dot would be two spellings
/// of one idea, and the eye reads punctuation as meaning.
pub const NAME_PLACE_SEPARATOR: &str = " · ";

/// A tab's tip: its name, where the name came from, and where it is standing
/// (M140, mock-up 4197-4201).
///
/// `source` is `None` when no layer won — a tab showing the profile's default
/// title has no provenance to report, and inventing one ("Working folder" for a
/// tab that has never announced a folder) would be the tip lying about the one
/// thing it exists to explain. With neither a source nor a folder the second
/// line is not written at all, which leaves a one-line tip saying the name. That
/// is M141's rule applied inside a string rather than across one.
#[must_use]
pub fn tab_tip(name: &str, source: Option<NameSource>, cwd: Option<&str>, pinned: bool) -> String {
    let mut tip = name.to_owned();
    let provenance = match (source, cwd) {
        (Some(source), Some(cwd)) => Some(format!("{}{NAME_PLACE_SEPARATOR}{cwd}", source.label())),
        (Some(source), None) => Some(source.label().to_owned()),
        (None, Some(cwd)) => Some(cwd.to_owned()),
        (None, None) => None,
    };
    if let Some(provenance) = provenance {
        tip.push('\n');
        tip.push_str(&provenance);
    }
    if pinned {
        // F46's wording, and it earns its own line: it is a fact about the tab's
        // future rather than about what it is showing now.
        tip.push_str("\nPinned — restored next launch");
    }
    tip
}

/// What the mark slot says while a command is running (D38, mock-up 4124-4128).
///
/// Empty when there is nothing to report, which is how the mark hands the
/// question back to the tab it sits on — see [`TooltipAnchors::push`]. The
/// mock-up arrives at the same place by removing the attribute (4341-4343).
#[must_use]
pub fn mark_tip(progress: Option<ProgressState>, working: bool) -> String {
    let Some(progress) = progress else {
        // `.ticon.working` carries `title="Working"` and nothing else — no
        // ellipsis, because this is a state and not a running commentary.
        return if working {
            "Working".to_owned()
        } else {
            String::new()
        };
    };
    // `Math.max(0, Math.min(100, Math.round(p.pct || 0)))`: a reading outside the
    // scale is clamped rather than shown, and a kind that carries no reading at
    // all reports zero, exactly as `p.pct || 0` does.
    let percent = |value: Option<u8>| u32::from(value.unwrap_or(0)).min(100);
    match progress {
        // The one kind with no number to show: its arc has no length to mean
        // anything, so the tip says what is true instead.
        ProgressState::Indeterminate => "Working…".to_owned(),
        ProgressState::Normal(value) => format!("{}%", percent(Some(value))),
        ProgressState::Error(value) => format!("{}% — error", percent(value)),
        ProgressState::Paused(value) => format!("{}% — paused", percent(value)),
    }
}

/// The singleton: which anchor is settling, which is showing, and since when.
///
/// Modelled on `PeekHover`/`HyperlinkHover`, which already solve the same shape
/// — arm a clock on a subject, do not restart it while the subject holds still,
/// promote when it elapses. The difference is the fade: a tip that has appeared
/// keeps a second clock, because it owes frames for 90ms after it arrives.
///
/// Two states, each an anchor paired with the instant that governs it. Pairing
/// them is what makes the invariants structural rather than remembered: a
/// deadline with no subject and a subject with no deadline are both states this
/// host simply cannot be in, so no code has to defend against either — and an
/// unreachable defence is worse than none, because it quietly heals the bug a
/// test was written to catch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TooltipHost {
    /// The anchor the pointer is resting on, and when its tip comes due.
    settling: Option<(TooltipAnchorId, Instant)>,
    /// The anchor whose tip is on screen, and when it appeared — the fade's own
    /// epoch.
    showing: Option<(TooltipAnchorId, Instant)>,
}

impl TooltipHost {
    /// Track the anchor under the pointer. `None` means "nothing tippable", and
    /// every suppression the design asks for is spelled as `None` by the caller:
    /// a drag in flight, the anchor that owns an open menu (I94), the tab being
    /// renamed. Returns whether anything visible changed.
    ///
    /// Resting on the anchor that is *already* showing is not a new subject and
    /// must not re-arm anything — otherwise a hand that trembles on a button
    /// takes the tip down and puts it back up 380ms later, forever.
    pub fn observe(&mut self, anchor: Option<TooltipAnchorId>, now: Instant) -> bool {
        if anchor.is_some() && anchor == self.active() {
            self.settling = None;
            return false;
        }
        // M142: the pointer left the host, so the tip goes at once and the timer
        // goes with it. Not a fade-out — the mock-up's `.tip` transitions on the
        // way in and simply loses `.show` on the way out.
        let hidden = self.showing.take().is_some();
        match anchor {
            Some(anchor) => {
                if self.settling.map(|(id, _)| id) != Some(anchor) {
                    self.settling = Some((anchor, now + TOOLTIP_DELAY));
                }
            }
            None => self.settling = None,
        }
        hidden
    }

    /// Promote a candidate whose delay has elapsed. Returns whether it did.
    pub fn activate_if_due(&mut self, now: Instant) -> bool {
        let Some((anchor, due)) = self.settling else {
            return false;
        };
        if now < due {
            return false;
        }
        self.settling = None;
        self.showing = Some((anchor, now));
        true
    }

    /// Forget whatever this host is pointing at that no longer exists. Returns
    /// whether a *visible* tip was taken down.
    ///
    /// Both states, not just the showing one, and that is the whole point: a tab
    /// closed during the 380ms wait would otherwise leave a candidate that
    /// matures into a tip with no anchor — a box that cannot be laid out, cannot
    /// be painted, and cannot stop asking for the frame it will never manage to
    /// draw. "The thing it describes is still there" is one condition and it
    /// applies to a tip that is coming as much as to one that has arrived.
    pub fn retain(&mut self, exists: impl Fn(TooltipAnchorId) -> bool) -> bool {
        if self.settling.is_some_and(|(id, _)| !exists(id)) {
            self.settling = None;
        }
        if self.showing.is_some_and(|(id, _)| !exists(id)) {
            return self.showing.take().is_some();
        }
        false
    }

    /// Take the tip down and disarm the clock — any press, the window losing
    /// focus, a menu opening (M142, I94). Returns whether anything was visible.
    pub fn hide(&mut self) -> bool {
        self.settling = None;
        self.showing.take().is_some()
    }

    /// The anchor whose tip is on screen.
    #[must_use]
    pub fn active(&self) -> Option<TooltipAnchorId> {
        self.showing.map(|(anchor, _)| anchor)
    }

    /// The next instant this host has something to do: the settle deadline while
    /// one is armed, the next frame of the fade while one is running.
    ///
    /// Handed to the loop's `earliest_deadline`, so a window with a tip settling
    /// wakes exactly when it is due and a window without one costs nothing.
    #[must_use]
    pub fn deadline(&self, now: Instant, motion: Motion, frame: Duration) -> Option<Instant> {
        if let Some((_, due)) = self.settling {
            return Some(due);
        }
        self.is_fading(now, motion).then(|| now + frame)
    }

    /// Whether the fade is still running, and therefore still owes frames.
    #[must_use]
    pub fn is_fading(&self, now: Instant, motion: Motion) -> bool {
        if motion == Motion::Reduced {
            return false;
        }
        self.showing
            .is_some_and(|(_, shown)| now.duration_since(shown) < TOOLTIP_FADE)
    }

    /// How solid the tip is drawn this frame — `opacity 0 -> 1` over
    /// [`TOOLTIP_FADE`] on the mock-up's own `ease`.
    ///
    /// Reduced motion gets the end state immediately. The mock-up's own
    /// reduced-motion block does not name `.tip`, but every other transition in
    /// this window stands down when the system asks for stillness, and a tip is
    /// the one popup you summon by *not moving* — a fade-in is exactly the kind
    /// of unrequested motion the preference is about.
    #[must_use]
    pub fn opacity(&self, now: Instant, motion: Motion) -> f32 {
        let Some((_, shown)) = self.showing else {
            return 0.0;
        };
        if motion == Motion::Reduced {
            return 1.0;
        }
        let elapsed = now.duration_since(shown).as_secs_f32();
        let progress = (elapsed / TOOLTIP_FADE.as_secs_f32()).clamp(0.0, 1.0);
        cubic_bezier(progress, EASE)
    }
}

/// A placed tip: the box, and the row each line of text sits in.
#[derive(Clone, Debug, PartialEq)]
pub struct TooltipLayout {
    /// `[left, top, right, bottom]`, physical pixels.
    pub frame: [f32; 4],
    /// One row per line, in order, each the full inner width.
    pub lines: Vec<([f32; 4], String)>,
}

/// Place the tip against its host (M139).
///
/// `line_widths` is the measured width of each line — only the font knows how
/// wide a string is, so the caller measures and this decides.
///
/// Horizontal: centred on the host, then clamped so neither edge comes within
/// the gap of the window's. Vertical: below the host by the gap, flipping above
/// when the bottom would not clear the window's own margin.
///
/// The flip has no second guard for a tip that fits neither above nor below, and
/// that is the mock-up's arithmetic exactly (8701-8702). Adding one would be
/// inventing a rule for a case this window cannot reach: every anchor lives in a
/// title bar 46 logical pixels tall, and the tallest tip is three lines.
#[must_use]
pub fn place(
    host: [f32; 4],
    line_widths: &[f32],
    window: (f32, f32),
    scale: f32,
) -> Option<([f32; 4], f32, f32)> {
    if line_widths.is_empty() {
        return None;
    }
    let px = |logical: f32| logical * scale;
    let pad_x = px(TIP_PADDING_X_LOGICAL_PX);
    let pad_y = px(TIP_PADDING_Y_LOGICAL_PX);
    let border = px(TIP_BORDER_LOGICAL_PX);
    let gap = px(TIP_GAP_LOGICAL_PX);
    let line_height = (px(TIP_FONT_LOGICAL_PX) * CHROME_LINE_HEIGHT).round();

    let text_width = line_widths.iter().copied().fold(0.0_f32, f32::max);
    let width = (text_width + 2.0 * (pad_x + border)).round();
    let height = (line_widths.len() as f32 * line_height + 2.0 * (pad_y + border)).round();

    let (window_width, window_height) = window;
    let centred = (host[0] + host[2]) / 2.0 - width / 2.0;
    let left = centred.min(window_width - width - gap).max(gap).round();

    let below = host[3] + gap;
    let top = if below + height > window_height - gap {
        host[1] - height - gap
    } else {
        below
    }
    .round();

    Some(([left, top, left + width, top + height], line_height, border))
}

/// The line box every other piece of chrome text is laid out in — see
/// `shape_chrome_labels`, which sizes a label's buffer to `font_size * 1.4`.
///
/// The mock-up leaves `.tip` on the document's inherited `line-height: normal`,
/// which is the *face's* own metric and therefore not a number the mock-up
/// states. Borrowing the one this renderer already uses everywhere is what keeps
/// a two-line tip's rows agreeing with every single-line label beside it; a
/// third number invented here would only be a guess at what Segoe happens to
/// report.
const CHROME_LINE_HEIGHT: f32 = 1.4;

/// Lay the tip out: the box, and one row per line.
///
/// The mock-up's `.tip` is `white-space: pre-line` and has no width bound, so a
/// line never wraps — it shrink-wraps to the longest one. That is why this
/// splits on `\n` and nothing else, and why no line-breaking lives here.
#[must_use]
pub fn layout(
    text: &str,
    host: [f32; 4],
    line_widths: &[f32],
    window: (f32, f32),
    scale: f32,
) -> Option<TooltipLayout> {
    let (frame, line_height, border) = place(host, line_widths, window, scale)?;
    let pad_x = TIP_PADDING_X_LOGICAL_PX * scale;
    let pad_y = TIP_PADDING_Y_LOGICAL_PX * scale;
    let lines = text
        .split('\n')
        .enumerate()
        .map(|(row, line)| {
            let top = frame[1] + border + pad_y + row as f32 * line_height;
            (
                [
                    frame[0] + border + pad_x,
                    top,
                    frame[2] - border - pad_x,
                    top + line_height,
                ],
                line.to_owned(),
            )
        })
        .collect();
    Some(TooltipLayout { frame, lines })
}

/// Paint the tip — one layer, always the last one handed to the renderer.
///
/// `z-index: 60` against the menu's `30` (mock-up 1207 and the note at 7339):
/// the tip is the only thing in this window that is *never* covered, because it
/// is the only thing that exists to explain what is under it.
#[must_use]
pub fn build(
    layout: &TooltipLayout,
    palette: &ChromePalette,
    scale: f32,
    opacity: f32,
) -> Vec<OverlayLayer> {
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut quads: Vec<OverlayQuad> = Vec::new();

    push_float_window(
        &mut quads,
        layout.frame,
        px(TIP_RADIUS_LOGICAL_PX),
        px(TIP_BORDER_LOGICAL_PX),
        px(bt_render::FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.tip_shadow_inner_alpha),
        alpha(palette.tip_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );

    let labels = layout
        .lines
        .iter()
        .map(|(rect, text)| ChromeLabel {
            text: text.clone(),
            rect: *rect,
            font_size_px: px(TIP_FONT_LOGICAL_PX),
            // `color: var(--ink2)` over `--menu` — the same ink a menu row that is
            // not the selected one is drawn in, which is what `--ink2` on that
            // surface already means.
            color: palette.menu_item_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        })
        .collect();

    vec![OverlayLayer {
        quads,
        labels,
        sprites: Vec::new(),
        opacity,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE: f32 = 1.0;
    const WINDOW: (f32, f32) = (1000.0, 700.0);

    fn host(left: f32, top: f32, right: f32, bottom: f32) -> [f32; 4] {
        [left, top, right, bottom]
    }

    // ── M139: placement ────────────────────────────────────────────────────

    #[test]
    fn a_tip_centres_on_its_host_and_stands_six_pixels_below_it() {
        let anchor = host(400.0, 10.0, 460.0, 40.0);
        let (frame, ..) = place(anchor, &[50.0], WINDOW, SCALE).expect("a tip is placed");
        let tip_centre = (frame[0] + frame[2]) / 2.0;
        let host_centre = (anchor[0] + anchor[2]) / 2.0;
        assert!(
            (tip_centre - host_centre).abs() <= 0.5,
            "centred: {tip_centre} vs {host_centre}"
        );
        assert!((frame[1] - (anchor[3] + 6.0)).abs() < 0.001, "{}", frame[1]);
    }

    #[test]
    fn a_tip_near_an_edge_is_pushed_in_to_six_pixels_and_no_further() {
        // Hard against the left: centring would put it off-window.
        let (left_frame, ..) = place(host(0.0, 10.0, 20.0, 40.0), &[200.0], WINDOW, SCALE).unwrap();
        assert!((left_frame[0] - 6.0).abs() < 0.001, "{}", left_frame[0]);

        // Hard against the right — the caption buttons live here, so this is the
        // case every window-control tip actually takes.
        let (right_frame, ..) =
            place(host(980.0, 10.0, 1000.0, 40.0), &[200.0], WINDOW, SCALE).unwrap();
        assert!(
            (right_frame[2] - (WINDOW.0 - 6.0)).abs() < 0.001,
            "{}",
            right_frame[2]
        );
    }

    #[test]
    fn a_tip_with_no_room_below_flips_above_its_host() {
        let anchor = host(400.0, 660.0, 460.0, 690.0);
        let (frame, ..) = place(anchor, &[50.0], WINDOW, SCALE).expect("a tip is placed");
        assert!(
            (frame[3] - (anchor[1] - 6.0)).abs() < 0.001,
            "sits above: {frame:?}"
        );
    }

    /// The box grows with the number of lines, and the padding is spent on both
    /// sides of both axes. A tab's tip is two lines by design and a pinned tab's
    /// is three, so this is the common case and not the exotic one.
    #[test]
    fn the_box_wraps_its_longest_line_and_stacks_the_rest() {
        let anchor = host(400.0, 10.0, 460.0, 40.0);
        let (one, line_height, border) = place(anchor, &[50.0], WINDOW, SCALE).unwrap();
        let (two, ..) = place(anchor, &[50.0, 120.0], WINDOW, SCALE).unwrap();

        // Width answers to the widest line, never the first or the last.
        assert!((one[2] - one[0] - (50.0 + 2.0 * (7.0 + 1.0))).abs() < 1.0);
        assert!((two[2] - two[0] - (120.0 + 2.0 * (7.0 + 1.0))).abs() < 1.0);
        // Height answers to the count.
        assert!(((two[3] - two[1]) - (one[3] - one[1]) - line_height).abs() < 1.0);
        assert!((border - 1.0).abs() < 0.001);
    }

    #[test]
    fn every_line_gets_its_own_row_inside_the_padding() {
        let anchor = host(400.0, 10.0, 460.0, 40.0);
        let laid = layout("first\nsecond", anchor, &[40.0, 60.0], WINDOW, SCALE)
            .expect("a two-line tip is laid out");
        assert_eq!(laid.lines.len(), 2);
        assert_eq!(laid.lines[0].1, "first");
        assert_eq!(laid.lines[1].1, "second");
        // Rows stack without overlapping, and both stay inside the frame.
        assert!(laid.lines[0].0[3] <= laid.lines[1].0[1] + 0.001);
        assert!(laid.lines[0].0[1] >= laid.frame[1]);
        assert!(laid.lines[1].0[3] <= laid.frame[3] + 0.001);
        // The text box is inset by the border and the horizontal padding.
        assert!((laid.lines[0].0[0] - (laid.frame[0] + 1.0 + 7.0)).abs() < 0.001);
    }

    // ── M141: an anchor with nothing to say is not an anchor ────────────────

    #[test]
    fn an_anchor_with_no_text_is_never_registered_and_falls_through_to_its_parent() {
        let mut anchors = TooltipAnchors::default();
        // The idle mark, inside the tab: pushed first because it is innermost.
        anchors.push(TooltipAnchorId::TabIcon(0), [10.0, 0.0, 30.0, 40.0], "");
        anchors.push(
            TooltipAnchorId::Tab(0),
            [0.0, 0.0, 200.0, 40.0],
            "bash\nWorking folder · /tmp",
        );

        let hit = anchors.at(20.0, 20.0).expect("the pointer is on the mark");
        assert_eq!(
            hit.id,
            TooltipAnchorId::Tab(0),
            "an idle mark hands the question to its tab"
        );
    }

    #[test]
    fn a_child_with_something_to_say_answers_before_the_tab_it_sits_in() {
        let mut anchors = TooltipAnchors::default();
        anchors.push(TooltipAnchorId::TabIcon(0), [10.0, 0.0, 30.0, 40.0], "42%");
        anchors.push(
            TooltipAnchorId::Tab(0),
            [0.0, 0.0, 200.0, 40.0],
            "bash\nWorking folder · /tmp",
        );

        assert_eq!(
            anchors.at(20.0, 20.0).map(|a| a.id),
            Some(TooltipAnchorId::TabIcon(0))
        );
        // …and the tab still answers everywhere the child is not.
        assert_eq!(
            anchors.at(100.0, 20.0).map(|a| a.id),
            Some(TooltipAnchorId::Tab(0))
        );
    }

    #[test]
    fn a_blank_string_is_no_more_a_tip_than_an_empty_one() {
        let mut anchors = TooltipAnchors::default();
        anchors.push(TooltipAnchorId::Settings, [0.0, 0.0, 10.0, 10.0], "   ");
        assert_eq!(anchors.at(5.0, 5.0), None);
        assert_eq!(anchors.find(TooltipAnchorId::Settings), None);
    }

    // ── M140 / D38: what the strings say ───────────────────────────────────

    /// The mock-up's `tabTip` (4197-4201), line for line. These are the
    /// user-facing words and they are copied, not paraphrased.
    #[test]
    fn a_tabs_tip_names_it_then_says_where_the_name_came_from() {
        assert_eq!(
            tab_tip(
                "claude",
                Some(NameSource::Manual),
                Some("C:\\src\\app"),
                false
            ),
            "claude\nNamed by you · C:\\src\\app"
        );
        assert_eq!(
            tab_tip(
                "npm run dev",
                Some(NameSource::Program),
                Some("C:\\src"),
                false
            ),
            "npm run dev\nSet by the program · C:\\src"
        );
        assert_eq!(
            tab_tip("app", Some(NameSource::Cwd), Some("C:\\src\\app"), false),
            "app\nWorking folder · C:\\src\\app"
        );
    }

    /// F46's wording, on its own line, and only when it is true.
    #[test]
    fn a_pinned_tab_says_it_will_come_back() {
        let pinned = tab_tip("app", Some(NameSource::Cwd), Some("C:\\src"), true);
        assert_eq!(
            pinned,
            "app\nWorking folder · C:\\src\nPinned — restored next launch"
        );
        assert_eq!(pinned.lines().count(), 3);
        assert!(!tab_tip("app", Some(NameSource::Cwd), Some("C:\\src"), false).contains("Pinned"));
    }

    /// A tab wearing the profile's default title has no provenance, and the tip
    /// must not manufacture one. It says the name and stops.
    #[test]
    fn a_tab_with_nothing_to_report_says_only_its_name() {
        assert_eq!(tab_tip("PowerShell", None, None, false), "PowerShell");
        assert_eq!(
            tab_tip("PowerShell", None, None, true),
            "PowerShell\nPinned — restored next launch"
        );
        // A folder with no winning layer is still a place worth naming.
        assert_eq!(
            tab_tip("PowerShell", None, Some("C:\\src"), false),
            "PowerShell\nC:\\src"
        );
    }

    #[test]
    fn the_mark_reports_the_run_and_stays_silent_otherwise() {
        assert_eq!(mark_tip(None, false), "", "an idle mark is not an anchor");
        assert_eq!(mark_tip(None, true), "Working");
        assert_eq!(
            mark_tip(Some(ProgressState::Indeterminate), true),
            "Working…"
        );
        assert_eq!(mark_tip(Some(ProgressState::Normal(42)), true), "42%");
        assert_eq!(
            mark_tip(Some(ProgressState::Error(Some(80))), true),
            "80% — error"
        );
        assert_eq!(
            mark_tip(Some(ProgressState::Paused(Some(15))), true),
            "15% — paused"
        );
        // `p.pct || 0` — a kind that carries no reading still reports a number.
        assert_eq!(
            mark_tip(Some(ProgressState::Error(None)), true),
            "0% — error"
        );
        assert_eq!(
            mark_tip(Some(ProgressState::Paused(None)), true),
            "0% — paused"
        );
        // `Math.min(100, …)` — a shell that reports past the end of the scale.
        assert_eq!(mark_tip(Some(ProgressState::Normal(200)), true), "100%");
        // The ring outranks the breath: what you are pointing at is the ring.
        assert_eq!(mark_tip(Some(ProgressState::Normal(7)), false), "7%");
    }

    // ── M137 / M142: the two clocks ────────────────────────────────────────

    #[test]
    fn a_tip_waits_three_hundred_and_eighty_milliseconds_and_not_a_moment_less() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Settings), start);

        assert!(!host.activate_if_due(start + Duration::from_millis(379)));
        assert_eq!(host.active(), None);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));
        assert_eq!(host.active(), Some(TooltipAnchorId::Settings));
    }

    #[test]
    fn resting_on_a_showing_tip_does_not_restart_its_clock() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Settings), start);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));

        // A hand that trembles on the button reports the same anchor again.
        let changed = host.observe(Some(TooltipAnchorId::Settings), start + TOOLTIP_DELAY);
        assert!(!changed, "nothing changed");
        assert_eq!(
            host.active(),
            Some(TooltipAnchorId::Settings),
            "the tip stays up"
        );
        assert_eq!(
            host.deadline(
                start + TOOLTIP_DELAY,
                Motion::Reduced,
                Duration::from_millis(16)
            ),
            None
        );
    }

    #[test]
    fn moving_to_a_new_anchor_takes_the_old_tip_down_and_starts_over() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Settings), start);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));

        let moved = start + TOOLTIP_DELAY + Duration::from_millis(1);
        assert!(host.observe(Some(TooltipAnchorId::Minimize), moved));
        assert_eq!(host.active(), None, "the old tip is gone at once");
        // And the new one waits its own full delay rather than inheriting.
        assert!(!host.activate_if_due(moved + Duration::from_millis(379)));
        assert!(host.activate_if_due(moved + TOOLTIP_DELAY));
        assert_eq!(host.active(), Some(TooltipAnchorId::Minimize));
    }

    /// M142: leaving the host hides *and* disarms. Hiding without clearing the
    /// timer is the bug where the tip lands 380ms later over nothing.
    #[test]
    fn leaving_an_anchor_clears_the_timer_as_well_as_the_tip() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Settings), start);
        host.observe(None, start + Duration::from_millis(10));

        // Asked *before* anything is polled: the clock has to be gone the moment
        // the pointer leaves, not merely gone by the time something notices. Poll
        // first and a host that quietly self-heals on the way past looks identical
        // to one that never armed.
        assert_eq!(
            host.deadline(start, Motion::Full, Duration::from_millis(16)),
            None,
            "a disarmed host asks for no wakeups"
        );
        assert!(!host.activate_if_due(start + Duration::from_secs(5)));
        assert_eq!(host.active(), None);
    }

    #[test]
    fn a_press_or_a_lost_window_takes_the_tip_down_immediately() {
        for settle in [false, true] {
            let mut host = TooltipHost::default();
            let start = Instant::now();
            host.observe(Some(TooltipAnchorId::Settings), start);
            if settle {
                assert!(host.activate_if_due(start + TOOLTIP_DELAY));
            }
            assert_eq!(host.hide(), settle, "reports whether anything was visible");
            assert_eq!(host.active(), None);
            // And it does not come back on its own.
            assert!(!host.activate_if_due(start + Duration::from_secs(5)));
        }
    }

    /// A tab closed while its tip was counting down must take the countdown with
    /// it. Left behind, the candidate matures into a tip with no anchor: nothing
    /// to lay out, nothing to paint, and a frame debt that can never be settled.
    #[test]
    fn a_subject_that_leaves_takes_its_pending_tip_with_it() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Tab(3)), start);

        // The tab closes 100ms into the wait.
        assert!(
            !host.retain(|id| id != TooltipAnchorId::Tab(3)),
            "nothing was visible yet"
        );
        assert_eq!(
            host.deadline(start, Motion::Full, Duration::from_millis(16)),
            None
        );
        assert!(!host.activate_if_due(start + Duration::from_secs(5)));
        assert_eq!(host.active(), None);
    }

    #[test]
    fn a_subject_that_leaves_takes_its_showing_tip_with_it() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Tab(3)), start);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));

        assert!(
            host.retain(|id| id != TooltipAnchorId::Tab(3)),
            "a visible tip came down"
        );
        assert_eq!(host.active(), None);
        // And a subject that is still there is left entirely alone.
        host.observe(Some(TooltipAnchorId::Tab(1)), start);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));
        assert!(!host.retain(|_| true));
        assert_eq!(host.active(), Some(TooltipAnchorId::Tab(1)));
    }

    #[test]
    fn an_armed_host_asks_to_be_woken_exactly_when_the_delay_is_up() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Settings), start);
        assert_eq!(
            host.deadline(start, Motion::Full, Duration::from_millis(16)),
            Some(start + TOOLTIP_DELAY)
        );
    }

    // ── M136: the fade ─────────────────────────────────────────────────────

    #[test]
    fn the_tip_fades_in_over_ninety_milliseconds_and_owes_frames_while_it_does() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Settings), start);
        let shown = start + TOOLTIP_DELAY;
        assert!(host.activate_if_due(shown));

        assert!(
            (host.opacity(shown, Motion::Full) - 0.0).abs() < 0.001,
            "starts invisible"
        );
        let middle = host.opacity(shown + Duration::from_millis(45), Motion::Full);
        assert!(middle > 0.0 && middle < 1.0, "climbing: {middle}");
        assert!((host.opacity(shown + TOOLTIP_FADE, Motion::Full) - 1.0).abs() < 0.001);
        assert!((host.opacity(shown + Duration::from_secs(9), Motion::Full) - 1.0).abs() < 0.001);

        // While it climbs it owes the next frame; once landed it owes nothing.
        let frame = Duration::from_millis(16);
        assert!(host.is_fading(shown + Duration::from_millis(45), Motion::Full));
        assert_eq!(
            host.deadline(shown + Duration::from_millis(45), Motion::Full, frame),
            Some(shown + Duration::from_millis(45) + frame)
        );
        assert!(!host.is_fading(shown + TOOLTIP_FADE, Motion::Full));
        assert_eq!(
            host.deadline(shown + TOOLTIP_FADE, Motion::Full, frame),
            None
        );
    }

    /// It is `ease` and not a straight ramp — the mock-up names the keyword, and
    /// `ease` leaves quickly and arrives slowly.
    #[test]
    fn the_fade_follows_the_mockups_own_ease_curve() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Settings), start);
        let shown = start + TOOLTIP_DELAY;
        host.activate_if_due(shown);
        let half = host.opacity(shown + TOOLTIP_FADE / 2, Motion::Full);
        assert!(
            half > 0.55,
            "ease is ahead of linear at the midpoint: {half}"
        );
    }

    #[test]
    fn stillness_skips_the_fade_and_owes_nothing() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some(TooltipAnchorId::Settings), start);
        let shown = start + TOOLTIP_DELAY;
        host.activate_if_due(shown);

        assert!(
            (host.opacity(shown, Motion::Reduced) - 1.0).abs() < 0.001,
            "there at once"
        );
        assert!(!host.is_fading(shown, Motion::Reduced));
        assert_eq!(
            host.deadline(shown, Motion::Reduced, Duration::from_millis(16)),
            None,
            "a still tip asks for no animation frames"
        );
    }

    #[test]
    fn a_host_with_nothing_showing_is_fully_transparent() {
        let host = TooltipHost::default();
        assert!((host.opacity(Instant::now(), Motion::Full) - 0.0).abs() < 0.001);
        assert!((host.opacity(Instant::now(), Motion::Reduced) - 0.0).abs() < 0.001);
    }

    // ── the painted layer ──────────────────────────────────────────────────

    #[test]
    fn the_tip_paints_one_layer_carrying_its_own_opacity_and_one_label_per_line() {
        let palette = bt_render::chrome_palette();
        let laid = layout(
            "bash\nWorking folder · /tmp",
            host(400.0, 10.0, 460.0, 40.0),
            &[40.0, 160.0],
            WINDOW,
            SCALE,
        )
        .unwrap();
        let layers = build(&laid, &palette, SCALE, 0.4);
        assert_eq!(layers.len(), 1, "a tip is one layer");
        let layer = &layers[0];
        assert!((layer.opacity - 0.4).abs() < 0.001);
        assert_eq!(layer.labels.len(), 2);
        assert_eq!(layer.labels[0].text, "bash");
        assert_eq!(layer.labels[1].text, "Working folder · /tmp");
        assert_eq!(layer.labels[0].font_size_px, 11.0);
        assert_eq!(layer.labels[0].color, palette.menu_item_text);
        assert!(layer.sprites.is_empty(), "a tip is words and a box");
        // Lift, hairline and face: the box reaches past its own frame on every
        // side, and its face is the menu's.
        assert!(
            layer.quads.iter().any(|quad| quad.rect[1] < laid.frame[1]),
            "lifted"
        );
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.menu_surface),
            "faced"
        );
    }
}
