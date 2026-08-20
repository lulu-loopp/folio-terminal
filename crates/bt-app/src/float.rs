//! The floating small window — `.float-win` — and the file tree that is its
//! first tenant.
//!
//! # A form, not a feature
//!
//! `DESIGN.md` §7.1 lists seven containers a piece of content can be put in, and
//! the third of them is 小窗 — a floating layer inside this same window. The
//! mock-up builds two of those (the files flyout and, later, the preview float)
//! out of one chassis and says why at line 688: **§7.0: 小窗是一个形态,骨架只有
//! 一份**. So this module owns the chassis, and the files tree is the first thing
//! shown inside it, not the thing it is for. The preview block inherits the head,
//! the foot, the grip and the whole placement/clamp/drag story by filling in a
//! different body — first come, first served, which is the whole reason this
//! slice builds it.
//!
//! # Two modes, an escalation ladder
//!
//! §7.1.2 and mock-up 3742-3751 give the form exactly two states, and the
//! difference between them is a *promise*:
//!
//! * **Peek** — summoned by hover-intent, dismissed by a timer when the pointer
//!   leaves. It never takes the keyboard and never switches tabs, because
//!   hovering is a question and questions do not commit
//!   (`DESIGN.md` §7.1「hover 原则:查询免提交」). Nobody promised it would stay.
//! * **Pinned** — summoned by a *click*, which **tears it off** the header into a
//!   free-floating window. Clicking away does not close it: you clicked to keep
//!   it, and then you clicked into the terminal to work beside it. It is closed
//!   by `×`, by Esc while it is the top layer, by Dock, or by pressing the same
//!   trigger again — and by nothing else, geometry included.
//!
//! `M2-tiny-window-priority.md` §3.2 names those two regimes TRANSIENT and
//! PINNED and rules on what each does when the window gets too small: a transient
//! float **dissolves**, and a pinned one is **re-clamped and never dissolved**,
//! down to an honest floor of its own header strip
//! ([`bt_render::FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX`]). That asymmetry is the same
//! sentence as the one above, read at a different size: dissolving something the
//! user asked for is a silent undo of their choice.
//!
//! # It is not a seat, and it does not touch the tree
//!
//! `M2-layout-solver-spec.md` §2.6.4 and `M2-tiny-window-priority.md` §3.1:
//! 浮窗**不是座位、不进树**. Everything here consumes the solved viewport as
//! **read-only** — it clamps itself into what `solve()` already produced and asks
//! for nothing back. The pinned test name that guards this from the other side is
//! `opening_or_moving_a_floating_surface_rewrites_no_ratio`
//! (`bt-layout/tests/pins.rs`). Which is also why a float's `{root, open, sel}`
//! is a **throwaway of its own** (G81) rather than a leaf's: the persistent layout
//! must not be able to tell that any of this happened.

use std::time::{Duration, Instant};

use bt_render::{
    ChromeLabel, ChromeLabelWeight, ChromePalette, FLOAT_WINDOW_ANIMATION_MS,
    FLOAT_WINDOW_BORDER_LOGICAL_PX, FLOAT_WINDOW_DRAG_MARGIN_LOGICAL_PX,
    FLOAT_WINDOW_FOOT_LOGICAL_PX, FLOAT_WINDOW_GRIP_LOGICAL_PX, FLOAT_WINDOW_HEAD_LOGICAL_PX,
    FLOAT_WINDOW_MAX_HEIGHT_LOGICAL_PX, FLOAT_WINDOW_MAX_HEIGHT_VIEWPORT_FRACTION,
    FLOAT_WINDOW_MIN_HEIGHT_LOGICAL_PX, FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX,
    FLOAT_WINDOW_MIN_WIDTH_LOGICAL_PX, FLOAT_WINDOW_RADIUS_LOGICAL_PX,
    FLOAT_WINDOW_RISE_LOGICAL_PX, FLOAT_WINDOW_SHADOW_LOGICAL_PX,
    FLOAT_WINDOW_TRIGGER_GAP_LOGICAL_PX, FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX,
    FLOAT_WINDOW_WIDTH_LOGICAL_PX, OverlayQuad,
};

use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::{LeafId, Motion, TabId};

/// `FLY_OPEN_MS` — how long the pointer must rest on a trigger before a peek is
/// summoned (§7.1.2, mock-up 3908).
pub const FLY_OPEN: Duration = Duration::from_millis(180);
/// `FLY_CLOSE_MS` — the grace after the pointer leaves the peek's region.
pub const FLY_CLOSE: Duration = Duration::from_millis(220);
/// `FLY_CLOSE_MS_LEFT` — the *longer* grace for a departure off the **left**
/// edge.
///
/// Not a rounding of the one above: a peek is left-aligned to the icon that
/// summoned it, so the trip in runs down that edge, and a little leftward drift
/// is almost always still heading in rather than leaving. Every other direction
/// is a decisive departure and gets the short one.
pub const FLY_CLOSE_LEFT: Duration = Duration::from_millis(420);
/// `PEEK_PAD` — how far past its own edge a peek still counts as "the pointer is
/// dealing with me" (§7.1.2「命中容差 8px」).
///
/// It bridges two real gaps at once: the six logical pixels between the trigger
/// and the window, and the antialiased edge where a float's own rounded corner
/// stops being opaque. Without it the peek closed while the pointer was crossing
/// the seam it had just been invited across.
pub const PEEK_PAD_LOGICAL_PX: f32 = 8.0;

/// The entrance and its reverse, one span for both (§7.1.2「进出动画 120ms」).
const FLOAT_ANIMATION: Duration = Duration::from_millis(FLOAT_WINDOW_ANIMATION_MS);

/// Which of the two promises this float is standing on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatMode {
    /// Hover summoned it and a timer will take it away.
    Peek,
    /// A click tore it off; only an explicit verb closes it.
    Pinned,
}

impl FloatMode {
    /// Whether this float is the TRANSIENT regime of
    /// `M2-tiny-window-priority.md` §3.2.
    #[must_use]
    pub fn is_transient(self) -> bool {
        matches!(self, Self::Peek)
    }
}

/// The header a float was summoned from — **an identity, never a rectangle**.
///
/// G86's lesson, and it cost a bug to learn: a background repaint inside the
/// 180ms intent window rebuilds the strip, and an intent that remembered *the
/// box* would be pointing at a box that no longer exists. Worse, a pointer that
/// is resting still never fires another hover to re-arm it, so the intent was
/// silently eaten and the peek simply never came. Remembering *whose* header it
/// was lets the trigger be found again in whatever the repaint built.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatTrigger {
    /// The folder button on a tab — which only exists while that tab holds
    /// exactly one terminal (H107), in the strip or in the rail. One trigger for
    /// both, because the mock-up builds both from one `flyoutTrigger()` and the
    /// action is the same action (H109).
    Tab(TabId),
    /// The folder button on a terminal pane's own head, in a split tab (H110).
    Pane(LeafId),
}

/// A float that has been torn off a docked column carries no trigger: the header
/// it would re-click is exactly the one that just stopped existing.
pub type FloatOrigin = Option<FloatTrigger>;

// ── geometry ────────────────────────────────────────────────────────────────

/// Every box of the chassis, in physical pixels of the whole surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatGeometry {
    /// The window itself, border included.
    pub frame: [f32; 4],
    /// `.fly-head`, the 30px strip that is also the drag handle when pinned.
    pub head: [f32; 4],
    /// The hairline under the head.
    pub head_edge: [f32; 4],
    /// The folder mark at the head's left.
    pub head_mark: [f32; 4],
    /// `.fly-root` — the room the root's last segment is laid out in.
    pub head_title: [f32; 4],
    /// The `DOCK` button, or `None` when the head is too narrow to seat it.
    pub dock: Option<[f32; 4]>,
    /// `.pv-dirty`'s **reserved** slot — occupied or not (P16/P47). `None` for a
    /// tenant that has no dirty bit to show, which is the files tree.
    pub dirty: Option<[f32; 4]>,
    /// `.pvf-save`, when the buffer inside is one that edits (P57).
    pub save: Option<[f32; 4]>,
    /// `.pvf-flip`, when the buffer inside is markdown.
    pub flip: Option<[f32; 4]>,
    /// The `×`.
    pub close: [f32; 4],
    /// What the tenant fills — the tree, for this slice.
    pub body: [f32; 4],
    /// The hairline over the foot.
    pub foot_edge: [f32; 4],
    /// `.fly-foot`, the whole clickable strip.
    pub foot: [f32; 4],
    /// The open-folder mark at the foot's left.
    pub foot_mark: [f32; 4],
    /// The room the full path is laid out in.
    pub foot_path: [f32; 4],
    /// `.fly-resize`, the 16×16 corner grip — pinned floats only.
    pub grip: Option<[f32; 4]>,
}

/// Snap a box to whole device pixels, so a hairline is one row of pixels rather
/// than two half-lit ones.
fn snap(rect: [f32; 4]) -> [f32; 4] {
    [
        rect[0].round(),
        rect[1].round(),
        rect[2].round(),
        rect[3].round(),
    ]
}

/// Which of the head's optional controls this tenant is asking for.
///
/// The files tree asks for none of them and gets the head it always had; a
/// preview asks for the dot it must reserve room for and for whichever of the
/// two buffer verbs its content class earns (P57's `editable` / `flippable`).
/// Passed to the *geometry* rather than only to the paint for D4's reason: a head
/// drawn with a button its layout never reserved is a button standing on the
/// name.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FloatHeadTools {
    /// Reserve `.pv-dirty`'s slot, occupied or not.
    pub dirty: bool,
    pub save: bool,
    pub flip: bool,
}

/// Lay the chassis out inside `frame`.
///
/// `dock_label_px` is the measured width of the button's caption, because only
/// the font knows it — the same division of labour the root menu and the drag
/// ghost already use. Pass `0.0` and the button collapses to its icon.
#[must_use]
pub fn float_geometry(
    frame: [f32; 4],
    mode: FloatMode,
    scale: f32,
    dock_label_px: f32,
    tools: FloatHeadTools,
) -> FloatGeometry {
    let px = |logical: f32| logical * scale;
    let border = px(FLOAT_WINDOW_BORDER_LOGICAL_PX).max(1.0).round();
    let inner = [
        frame[0] + border,
        frame[1] + border,
        frame[2] - border,
        frame[3] - border,
    ];
    let head_height = px(FLOAT_WINDOW_HEAD_LOGICAL_PX).round();
    let foot_height = px(FLOAT_WINDOW_FOOT_LOGICAL_PX).round();
    let head = snap([inner[0], inner[1], inner[2], inner[1] + head_height]);
    let head_edge = snap([head[0], head[3], head[2], head[3] + border]);
    // The head and the foot both keep their whole height even when the window
    // has been squeezed to the honest floor; what gives way is the body, which
    // is allowed to reach zero. A strip-height window is therefore all header,
    // which is exactly what §3.4 asks for — the `×` and the handle survive.
    let foot_top = (inner[3] - foot_height).max(head_edge[3]);
    let foot = snap([inner[0], foot_top, inner[2], inner[3]]);
    let foot_edge = snap([
        foot[0],
        (foot[1] - border).max(head_edge[3]),
        foot[2],
        foot[1],
    ]);
    let body = snap([
        inner[0],
        head_edge[3],
        inner[2],
        foot_edge[1].max(head_edge[3]),
    ]);

    let mark = px(FLOAT_HEAD_MARK_LOGICAL_PX);
    let gap = px(FLOAT_HEAD_GAP_LOGICAL_PX);
    let head_pad_left = px(FLOAT_HEAD_PADDING_LEFT_LOGICAL_PX);
    let head_pad_right = px(FLOAT_HEAD_PADDING_RIGHT_LOGICAL_PX);
    let centred = |box_height: f32, strip: [f32; 4]| {
        let top = strip[1] + ((strip[3] - strip[1]) - box_height) / 2.0;
        [top, top + box_height]
    };
    let [mark_top, mark_bottom] = centred(mark, head);
    let head_mark = snap([
        head[0] + head_pad_left,
        mark_top,
        head[0] + head_pad_left + mark,
        mark_bottom,
    ]);

    let close_box = px(FLOAT_CLOSE_BOX_LOGICAL_PX);
    let [close_top, close_bottom] = centred(close_box, head);
    let close_right = head[2] - head_pad_right;
    let close = snap([
        close_right - close_box,
        close_top,
        close_right,
        close_bottom,
    ]);

    // The `DOCK` button, taken off the trailing run before the `×` does — the
    // pane head's own rule (`pane_head_geometry`), one surface over.
    let dock_box = px(FLOAT_DOCK_PADDING_X_LOGICAL_PX) * 2.0
        + px(FLOAT_DOCK_GLYPH_LOGICAL_PX)
        + px(FLOAT_DOCK_GAP_LOGICAL_PX)
        + dock_label_px;
    let dock_height = px(FLOAT_DOCK_HEIGHT_LOGICAL_PX);
    let [dock_top, dock_bottom] = centred(dock_height, head);
    // **Both edges floored, never rounded.** `dock_box` is padding and a gap —
    // whole numbers at any scale — plus a *measured* caption, which is
    // fractional almost always, and `close[0] - gap` is fractional at every
    // fractional scale. Snapping the two edges independently rounds one of them
    // inward half the time, and the half pixel it takes comes off the caption's
    // own room: the label's rect is this box less its two paddings, so a box a
    // fraction short is a caption clipped at its last letter. Flooring puts both
    // edges on whole device pixels — which is what `snap` was here for, and what
    // a rounded pill fill needs — while moving each of them in the one direction
    // that cannot lose ink: the box only grows, and the gap to the `×` only
    // widens.
    let dock_right = (close[0] - gap).floor();
    let dock_left = (dock_right - dock_box).floor();
    // A head with no room for the button does without it rather than drawing it
    // on top of the name: `×` is the one control that must never be crowded out,
    // because it is the only one that can undo the squeeze.
    let dock = (dock_left > head_mark[2] + gap)
        .then(|| [dock_left, dock_top.round(), dock_right, dock_bottom.round()]);

    // The two buffer verbs, taken off the trailing run in the mock-up's own DOM
    // order (P57: `.pvf-save`, `.pvf-flip`, `.fly-dock`, `.fly-close`), so a head
    // that has lost room drops them from the *left* of that run — the order
    // flexbox would drop them in, and the order that keeps `×` reachable longest.
    // The same rule `preview_head_geometry` follows one surface over.
    let button_box = px(FLOAT_DOCK_GLYPH_LOGICAL_PX) + px(FLOAT_DOCK_PADDING_X_LOGICAL_PX) * 2.0;
    let mut run_left = dock.map_or(close[0], |dock| dock[0]);
    let mut take = |wanted: bool| -> Option<[f32; 4]> {
        if !wanted {
            return None;
        }
        let right = (run_left - gap).floor();
        let left = (right - button_box).floor();
        (left > head_mark[2] + gap).then(|| {
            run_left = left;
            [left, dock_top.round(), right, dock_bottom.round()]
        })
    };
    let flip = take(tools.flip);
    let save = take(tools.save);

    // `.pv-dirty { width: 12px }` — a *reserved* slot beside the name, so the dot
    // appearing shoves nothing (P16). It is taken off the same trailing run and
    // before the name, which is where the DOM puts it.
    let dirty_slot = px(FLOAT_DIRTY_SLOT_LOGICAL_PX);
    let dirty = tools.dirty.then(|| {
        let right = (run_left - gap).floor();
        let left = (right - dirty_slot).floor();
        run_left = left;
        snap([left, head[1], right, head[3]])
    });

    let title_right = run_left - gap;
    let head_title = snap([
        head_mark[2] + gap,
        head[1],
        title_right.max(head_mark[2] + gap),
        head[3],
    ]);

    let foot_pad_left = px(FLOAT_FOOT_PADDING_LEFT_LOGICAL_PX);
    let foot_pad_right = px(FLOAT_FOOT_PADDING_RIGHT_LOGICAL_PX);
    let [foot_mark_top, foot_mark_bottom] = centred(mark, foot);
    let foot_mark = snap([
        foot[0] + foot_pad_left,
        foot_mark_top,
        foot[0] + foot_pad_left + mark,
        foot_mark_bottom,
    ]);
    let foot_path = snap([
        foot_mark[2] + gap,
        foot[1],
        (foot[2] - foot_pad_right).max(foot_mark[2] + gap),
        foot[3],
    ]);

    let grip_box = px(FLOAT_WINDOW_GRIP_LOGICAL_PX);
    let grip = (mode == FloatMode::Pinned)
        .then(|| snap([inner[2] - grip_box, inner[3] - grip_box, inner[2], inner[3]]));

    FloatGeometry {
        frame,
        head,
        head_edge,
        head_mark,
        head_title,
        dock,
        dirty,
        save,
        flip,
        close,
        body,
        foot_edge,
        foot,
        foot_mark,
        foot_path,
        grip,
    }
}

/// `.float-win .fly-head .files-ico { width: 13px }`, and the foot's mark too.
pub const FLOAT_HEAD_MARK_LOGICAL_PX: f32 = 13.0;
/// `.float-win .fly-head { gap: 6px }`.
pub const FLOAT_HEAD_GAP_LOGICAL_PX: f32 = 6.0;
/// `.float-win .fly-head { padding: 0 5px 0 10px }` — the left half.
pub const FLOAT_HEAD_PADDING_LEFT_LOGICAL_PX: f32 = 10.0;
/// The right half of the same declaration.
pub const FLOAT_HEAD_PADDING_RIGHT_LOGICAL_PX: f32 = 5.0;
/// `.float-win .fly-head { font-size: 11px }`.
pub const FLOAT_HEAD_FONT_LOGICAL_PX: f32 = 11.0;
/// `.float-win .fly-head { letter-spacing: .04em }`.
pub const FLOAT_HEAD_TRACKING_EM: f32 = 0.04;
/// `.float-win .fly-close { padding: 4px }` around a `9px` glyph.
pub const FLOAT_CLOSE_BOX_LOGICAL_PX: f32 = 17.0;
/// `.float-win .fly-close svg { width: 9px }`.
pub const FLOAT_CLOSE_GLYPH_LOGICAL_PX: f32 = 9.0;
/// `.float-win .fly-head button { padding: 3px 6px }` — the horizontal half.
pub const FLOAT_DOCK_PADDING_X_LOGICAL_PX: f32 = 6.0;
/// `.float-win .fly-head button svg { width: 13px }`.
pub const FLOAT_DOCK_GLYPH_LOGICAL_PX: f32 = 13.0;
/// `.float-win .fly-head button { gap: 4px }`.
pub const FLOAT_DOCK_GAP_LOGICAL_PX: f32 = 4.0;
/// The button's own box: `3px` of padding above and below its `13px` glyph.
pub const FLOAT_DOCK_HEIGHT_LOGICAL_PX: f32 = 19.0;
/// `.float-win .fly-head button { font-size: 10px }`.
pub const FLOAT_DOCK_FONT_LOGICAL_PX: f32 = 10.0;
/// `.float-win .fly-head button { border-radius: 5px }`, shared by the `×`.
pub const FLOAT_BUTTON_RADIUS_LOGICAL_PX: f32 = 5.0;
/// `.float-win .fly-foot { padding: 0 18px 0 10px }` — the left half.
pub const FLOAT_FOOT_PADDING_LEFT_LOGICAL_PX: f32 = 10.0;
/// The right half. Wider than the left, because the grip lives in that corner.
pub const FLOAT_FOOT_PADDING_RIGHT_LOGICAL_PX: f32 = 18.0;
/// `.float-win .fly-foot { font-size: 11px }`.
pub const FLOAT_FOOT_FONT_LOGICAL_PX: f32 = 11.0;
/// `.float-win .fly-resize::after` — the chevron's box inside the grip.
pub const FLOAT_GRIP_GLYPH_LOGICAL_PX: f32 = 8.0;
/// `right: 3px; bottom: 3px` — how far that chevron sits in from the corner.
pub const FLOAT_GRIP_GLYPH_INSET_LOGICAL_PX: f32 = 3.0;
/// `#pv-float .pv-dirty { width: 12px }` (P47) — the dot's reserved slot.
pub const FLOAT_DIRTY_SLOT_LOGICAL_PX: f32 = 12.0;
/// `#pv-float .pv-dirty { font-size: 13px }` — the glyph inside that slot, the
/// same size the docked head sets it at.
pub const FLOAT_DIRTY_GLYPH_LOGICAL_PX: f32 = crate::seats::PREVIEW_DIRTY_FONT_LOGICAL_PX;

// ── the preview float's own numbers (P45, P49, P65 — N36's "second set") ────
//
// The chassis is one skeleton with two sets of dimensions, and every one of these
// differs from the files flyout's on purpose. P49 lists the differences and says
// why in one line: **do not assume "shared chassis" means "same numbers"**.

/// `#pv-float { width: 430px }` — against the flyout's 264.
///
/// A file is read across, a folder is read down. 264 is a column of names; 430 is
/// the narrowest thing a line of source can be shown in without becoming a
/// thumbnail — the same argument `MIN_PREVIEW_W` makes for the docked pane.
pub const PREVIEW_FLOAT_WIDTH_LOGICAL_PX: f32 = 430.0;
/// `#pv-float { max-height: min(64vh, 520px) }` — the flat half.
pub const PREVIEW_FLOAT_MAX_HEIGHT_LOGICAL_PX: f32 = 520.0;
/// And the proportional half, against the flyout's `.62`.
pub const PREVIEW_FLOAT_MAX_HEIGHT_VIEWPORT_FRACTION: f32 = 0.64;
/// The grip's floor: `clamp(clientX - r0.left, 260, …)` (P65), against 200.
pub const PREVIEW_FLOAT_MIN_WIDTH_LOGICAL_PX: f32 = 260.0;
/// `clamp(clientY - r0.top, 200, …)`, against 150.
pub const PREVIEW_FLOAT_MIN_HEIGHT_LOGICAL_PX: f32 = 200.0;
/// How far a float born on top of another steps down and right (the 2026-08-12
/// cascade ruling).
///
/// One title bar's worth, which is what every stacking window manager since the
/// Macintosh has used and the smallest step at which the window underneath is
/// still visibly a *window* rather than a misprint.
pub const FLOAT_CASCADE_STEP_LOGICAL_PX: f32 = 24.0;
/// How close two origins have to be before the newcomer counts as landing on
/// top of the old one.
pub const FLOAT_CASCADE_TOLERANCE_LOGICAL_PX: f32 = 4.0;

/// The two sets of numbers, chosen by who is moving in.
///
/// A struct rather than eight `match`es scattered through the placement, the
/// grip and the re-clamp: "which tenant is this" is asked once, at the door, and
/// what comes back is the whole dimension set. Adding a third tenant adds a
/// constructor here and touches nothing else, which is the same extension seam
/// `SeatMetrics` gives the layout solver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatSizing {
    pub width: f32,
    pub max_height: f32,
    pub max_height_fraction: f32,
    pub min_width: f32,
    pub min_height: f32,
}

impl FloatSizing {
    /// The files flyout's set — 264 / min(62vh, 460) / 200×150.
    #[must_use]
    pub fn files() -> Self {
        Self {
            width: FLOAT_WINDOW_WIDTH_LOGICAL_PX,
            max_height: FLOAT_WINDOW_MAX_HEIGHT_LOGICAL_PX,
            max_height_fraction: FLOAT_WINDOW_MAX_HEIGHT_VIEWPORT_FRACTION,
            min_width: FLOAT_WINDOW_MIN_WIDTH_LOGICAL_PX,
            min_height: FLOAT_WINDOW_MIN_HEIGHT_LOGICAL_PX,
        }
    }

    /// The preview float's — 430 / min(64vh, 520) / 260×200.
    #[must_use]
    pub fn preview() -> Self {
        Self {
            width: PREVIEW_FLOAT_WIDTH_LOGICAL_PX,
            max_height: PREVIEW_FLOAT_MAX_HEIGHT_LOGICAL_PX,
            max_height_fraction: PREVIEW_FLOAT_MAX_HEIGHT_VIEWPORT_FRACTION,
            min_width: PREVIEW_FLOAT_MIN_WIDTH_LOGICAL_PX,
            min_height: PREVIEW_FLOAT_MIN_HEIGHT_LOGICAL_PX,
        }
    }
}

/// The size a float opens at, before any hand has touched it.
///
/// Width is the design's 264 flat. Height is the content's own, capped by
/// `min(62vh, 460px)` and floored at a strip — see
/// [`bt_render::FLOAT_WINDOW_MAX_HEIGHT_LOGICAL_PX`] for why the cap applies to
/// a fresh pinned window too.
#[must_use]
pub fn float_opening_size(
    content_height: f32,
    viewport: [f32; 4],
    scale: f32,
    sizing: FloatSizing,
) -> [f32; 2] {
    let px = |logical: f32| logical * scale;
    let margin = px(FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX);
    let cap = float_height_cap(viewport, scale, sizing);
    let floor = px(FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX);
    let width = px(sizing.width).min((viewport[2] - viewport[0] - margin * 2.0).max(floor));
    [
        width.round(),
        content_height.clamp(floor, cap.max(floor)).round(),
    ]
}

/// `max-height: min(62vh, 460px)` — the tallest a float of this tenant may open,
/// in physical pixels.
///
/// Extracted from [`float_opening_size`] rather than restated, because it is now
/// asked a second question: **how tall does a window open when its content is
/// not yet known?** A tree read on a worker has no rows on the frame it is
/// summoned (`place_float` builds every float with an empty `DirCache`), and a
/// window sized to the one `Loading` row standing in for them is the bare strip
/// in the corner the user reported on 2026-08-13. The kind's own maximum is the
/// honest opening size for an unknown amount of content: it is the biggest this
/// window is ever allowed to be, so no arriving row can make it *grow* — and
/// `FloatWin::self_sizing` shrinks it to the rows the moment they land, through
/// the one sizing path that was always there.
#[must_use]
pub fn float_height_cap(viewport: [f32; 4], scale: f32, sizing: FloatSizing) -> f32 {
    let px = |logical: f32| logical * scale;
    let margin = px(FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX);
    let viewport_height = (viewport[3] - viewport[1] - margin * 2.0).max(0.0);
    (viewport_height * sizing.max_height_fraction).min(px(sizing.max_height))
}

/// The natural height of a float whose body wants `body_height` pixels.
#[must_use]
pub fn float_height_for_body(body_height: f32, scale: f32) -> f32 {
    let px = |logical: f32| logical * scale;
    let border = px(FLOAT_WINDOW_BORDER_LOGICAL_PX).max(1.0).round();
    border * 2.0
        + px(FLOAT_WINDOW_HEAD_LOGICAL_PX).round()
        + px(FLOAT_WINDOW_FOOT_LOGICAL_PX).round()
        + border * 2.0
        + body_height.max(0.0)
}

/// Where a fresh float stands relative to the trigger that summoned it (G89).
///
/// Under the trigger, left-aligned to it, six pixels down; flipped **above** it
/// when there is no room below; and horizontally clamped — never flipped —
/// which is `peek_box_layout`'s rule and the shape
/// `M2-tiny-window-priority.md` §3.3 asks every float to copy: 主轴翻转、次轴
/// clamp.
///
/// # The third case, and why this returns a frame (user report 2026-08-12)
///
/// 主轴翻转 has an unstated premise — that one of the two sides *can* hold the
/// window. A trigger in the middle of the window breaks it: a float summoned
/// from the lower pane of an up/down split is taller than the room below **and**
/// taller than the room above, and the old shape answered that by flipping
/// above anyway, overflowing the top, and letting the vertical clamp haul it to
/// the viewport's edge. Left-aligned to a trigger that lives at the right end
/// of a pane head, the result was a window in the *window's* top-right corner
/// with nothing beneath it — a float that has visibly stopped belonging to the
/// control that opened it, which is the one thing a placement owes.
///
/// So the main axis is decided in three steps, not two: take the side that
/// fits, else the side with **more room**, and give the window that much
/// height. Shrinking is the honest answer where flipping has run out — every
/// other float in this file already accepts a smaller size rather than a wrong
/// place ([`clamp_pinned`], §3.4's strip floor), and 6px below a real head beats
/// the design height measured from somewhere the user was not looking. Which is
/// why the return value is the whole frame: the height is part of the answer.
#[must_use]
pub fn float_placement(
    trigger: [f32; 4],
    size: [f32; 2],
    viewport: [f32; 4],
    scale: f32,
) -> [f32; 4] {
    let px = |logical: f32| logical * scale;
    let margin = px(FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX);
    let gap = px(FLOAT_WINDOW_TRIGGER_GAP_LOGICAL_PX);
    let strip = px(FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX);
    let left_limit = viewport[0] + margin;
    let right_limit = viewport[2] - margin - size[0];
    let left = trigger[0].clamp(left_limit, right_limit.max(left_limit));
    let under = trigger[3] + gap;
    let room_under = (viewport[3] - margin - under).max(0.0);
    let room_over = (trigger[1] - gap - (viewport[1] + margin)).max(0.0);
    let (top, height) = if size[1] <= room_under {
        (under, size[1])
    } else if size[1] <= room_over {
        (trigger[1] - gap - size[1], size[1])
    } else if room_under >= room_over {
        (under, room_under.max(strip))
    } else {
        let height = room_over.max(strip);
        (trigger[1] - gap - height, height)
    };
    [
        left.round(),
        top.round(),
        (left + size[0]).round(),
        (top + height).round(),
    ]
}

/// Step a newborn window down and right until it is not standing exactly on top
/// of one that is already open (**user ruling, 2026-08-12**).
///
/// # Why a cascade and not a de-duplication
///
/// The morning of the same day repealed 同根去重: opening a second window on a
/// root you already have is legal and ordinary, because two windows on one folder
/// are two viewports rather than two copies. What that repeal left behind was the
/// symptom the de-duplication had been hiding — click the same trigger twice and
/// the second window lands on the first *to the pixel*, so the thing that
/// obviously happened (a window opened) is invisible, and the thing that obviously
/// did not (nothing) is what you see. Every stacking window manager since the
/// Macintosh answers this the same way, and the answer is not "refuse".
///
/// # The shape
///
/// One step of [`FLOAT_CASCADE_STEP_LOGICAL_PX`] per window already standing at
/// this origin, tried in order, **on whichever axes still have room**, and the
/// search rolls back to the un-stepped origin only when neither axis has any.
/// Rolling back rather than clamping: a clamped cascade piles every window after
/// the fourth against the same edge, which is the collision this exists to
/// prevent, arrived at from the other side.
///
/// **Per axis, and that is the correction of 2026-08-13.** This used to abandon
/// the whole ladder the moment the *diagonal* left the viewport, which reads as
/// the same rule until you notice where a preview float is born: popped out of a
/// pane head at the far right of the tree, so [`float_placement`] clamps it flush
/// against `viewport[2] - margin` and the sideways half of the very first step
/// has nowhere to go. The ladder was abandoned at step zero and the function
/// returned an origin it had just been told was occupied — two windows to the
/// pixel, the lower one unreachable, which is exactly the collision the ruling is
/// about. A window with no room to its right can still go *down*; one against the
/// bottom can still go right; only in the far corner is there genuinely nowhere
/// left, and that is where the roll-back belongs.
///
/// `taken` is every origin already occupied — the caller's own list, because this
/// module must not assume the newcomer is going into *this* host (a pop-out
/// computes its frame before the window exists).
#[must_use]
pub fn cascade_origin(
    frame: [f32; 4],
    taken: &[[f32; 2]],
    viewport: [f32; 4],
    scale: f32,
) -> [f32; 4] {
    let px = |logical: f32| logical * scale;
    let step = px(FLOAT_CASCADE_STEP_LOGICAL_PX);
    let tolerance = px(FLOAT_CASCADE_TOLERANCE_LOGICAL_PX);
    let margin = px(FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX);
    let width = frame[2] - frame[0];
    let height = frame[3] - frame[1];
    let occupied = |left: f32, top: f32| {
        taken.iter().any(|origin| {
            (origin[0] - left).abs() <= tolerance && (origin[1] - top).abs() <= tolerance
        })
    };
    let mut left = frame[0];
    let mut top = frame[1];
    // Bounded by the number of windows that can be standing here: each turn of
    // the loop is one of them, so a list of `n` origins cannot ask for more than
    // `n` steps, and the roll-back below ends it sooner than that.
    for _ in 0..=taken.len() {
        if !occupied(left, top) {
            break;
        }
        let room_right = left + step + width <= viewport[2] - margin;
        let room_down = top + step + height <= viewport[3] - margin;
        // The far corner: the ladder has run out of screen on both axes, so it
        // wraps to where the placement put it and accepts the overlap. Every
        // stacking window manager wraps eventually; what none of them does is
        // wrap on the first rung.
        if !room_right && !room_down {
            left = frame[0];
            top = frame[1];
            break;
        }
        if room_right {
            left += step;
        }
        if room_down {
            top += step;
        }
    }
    snap([left, top, left + width, top + height])
}

/// Put a pinned float back inside the viewport after the window changed shape
/// (G93, §7.1.2「主窗口缩小自动重钳位」).
///
/// **Translate first, shrink only if translating cannot do it**, and never below
/// the honest floor of one header strip
/// ([`bt_render::FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX`], §3.4). The order matters:
/// the ruling is 平移回视口、尺寸不变, so a window that still fits keeps every
/// pixel of the size its owner gave it, and only one that cannot fit at all is
/// allowed to lose any. And it is allowed to go under the 200×150 the grip
/// enforces — §7.1.2 says so in as many words — because the alternative is a
/// window stranded off-screen with its only handle out of reach.
#[must_use]
pub fn clamp_pinned(frame: [f32; 4], viewport: [f32; 4], scale: f32) -> [f32; 4] {
    let px = |logical: f32| logical * scale;
    let margin = px(FLOAT_WINDOW_DRAG_MARGIN_LOGICAL_PX);
    let strip = px(FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX);
    let available_width = (viewport[2] - viewport[0] - margin * 2.0).max(0.0);
    let available_height = (viewport[3] - viewport[1] - margin * 2.0).max(0.0);
    let width = (frame[2] - frame[0]).min(available_width.max(strip));
    let height = (frame[3] - frame[1]).min(available_height.max(strip));
    let left_limit = viewport[0] + margin;
    let top_limit = viewport[1] + margin;
    let left = frame[0].clamp(left_limit, (viewport[2] - margin - width).max(left_limit));
    let top = frame[1].clamp(top_limit, (viewport[3] - margin - height).max(top_limit));
    snap([left, top, left + width, top + height])
}

/// Move a pinned float under a dragged pointer, keeping the grab point under it.
///
/// The clamp is the *drag* margin, not the placement one: how far your own hand
/// may push a window against an edge is a different question from where the app
/// puts one you did not position.
#[must_use]
pub fn float_dragged_to(
    frame: [f32; 4],
    pointer: [f32; 2],
    grab: [f32; 2],
    viewport: [f32; 4],
    scale: f32,
) -> [f32; 4] {
    let px = |logical: f32| logical * scale;
    let margin = px(FLOAT_WINDOW_DRAG_MARGIN_LOGICAL_PX);
    let width = frame[2] - frame[0];
    let height = frame[3] - frame[1];
    let left_limit = viewport[0] + margin;
    let top_limit = viewport[1] + margin;
    let left =
        (pointer[0] - grab[0]).clamp(left_limit, (viewport[2] - margin - width).max(left_limit));
    let top =
        (pointer[1] - grab[1]).clamp(top_limit, (viewport[3] - margin - height).max(top_limit));
    snap([left, top, left + width, top + height])
}

/// Resize a pinned float from its bottom-right grip.
///
/// The floors here are the grip's own 200×150 (`.float-win.pinned`), which is a
/// different number from [`clamp_pinned`]'s strip and deliberately so: a hand
/// dragging the corner is not allowed to make the window useless, while a window
/// being squeezed by a shrinking screen is allowed to become a strip rather than
/// vanish. One is a limit on an action, the other a limit on a consequence.
#[must_use]
pub fn float_resized_to(
    frame: [f32; 4],
    pointer: [f32; 2],
    viewport: [f32; 4],
    scale: f32,
    sizing: FloatSizing,
) -> [f32; 4] {
    let px = |logical: f32| logical * scale;
    let margin = px(FLOAT_WINDOW_DRAG_MARGIN_LOGICAL_PX);
    let min_width = px(sizing.min_width);
    let min_height = px(sizing.min_height);
    let right = pointer[0]
        .min(viewport[2] - margin)
        .max(frame[0] + min_width);
    let bottom = pointer[1]
        .min(viewport[3] - margin)
        .max(frame[1] + min_height);
    snap([frame[0], frame[1], right, bottom])
}

/// Whether the pointer still counts as "dealing with this peek", and which way
/// it left if it did not (G83).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeekReach {
    /// Inside the window or within [`PEEK_PAD_LOGICAL_PX`] of it.
    pub inside: bool,
    /// Left of the window's left edge — the departure that gets the long grace.
    pub off_left: bool,
}

/// Test the pointer against a peek's own box plus its tolerance.
#[must_use]
pub fn peek_reach(frame: [f32; 4], x: f32, y: f32, scale: f32) -> PeekReach {
    let pad = PEEK_PAD_LOGICAL_PX * scale;
    PeekReach {
        inside: x >= frame[0] - pad
            && x <= frame[2] + pad
            && y >= frame[1] - pad
            && y <= frame[3] + pad,
        off_left: x < frame[0],
    }
}

/// What the pointer is over inside a float.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatPart {
    /// Bare header — the drag handle when pinned.
    Head,
    /// The `DOCK` button.
    Dock,
    /// `.pvf-save` — a preview float's own head control.
    Save,
    /// `.pvf-flip` — likewise.
    Flip,
    /// The `×`.
    Close,
    /// A row of whatever list is in the body, by visible index — the tree's, or
    /// the Git page's.
    Row(usize),
    /// **A verb on one row of a floating Git page** (user ruling, 2026-08-19).
    ///
    /// The chassis knows a git word here for [`crate::seats::ChromeTarget`]'s
    /// reason: a verb is *smaller* than the row it stands on, so "which button"
    /// has to be decided by the same hit test that decides "which row", or the
    /// row swallows the button. The tree has no such thing, which is why this
    /// variant never arises for one.
    GitAct {
        index: usize,
        act: crate::git_panel::GitAct,
    },
    /// **A row of a floating commit graph**, by index (user report, 2026-08-20).
    ///
    /// Its own variant and not [`Self::Row`] for [`crate::seats::ChromeTarget`]'s
    /// own reason, one host along: the two lists are two lists — a tree's rows
    /// and a graph's — and a press that could not say which would open the wrong
    /// row of whichever happened to be longer. The three that follow are the
    /// graph's parts that are *smaller* than a row, or outside the list
    /// altogether, and they are named here for [`Self::GitAct`]'s reason: what
    /// the pointer is on has to be decided by the hit test that decides which
    /// row, or the row swallows the button.
    GraphRow(usize),
    /// One of the graph toolbar's controls (T1) — outside the list, above it.
    GraphTool(crate::git_graph::GraphTool),
    /// One pressable part of the open commit's detail block (v2 ②).
    GraphDetail {
        index: usize,
        part: crate::git_graph::GraphDetailPart,
    },
    /// The body, but not on a row.
    Body,
    /// The foot, which reveals the folder in the OS file manager.
    Foot,
    /// The corner grip.
    Grip,
}

/// Resolve a pointer against the chassis, smallest target first.
///
/// `body` is the tenant's own hit test — `FilesTreeGeometry::row_at` for a tree,
/// the Git page's rows and verbs for a Git page — passed in rather than
/// performed here, because the chassis does not know what is inside it and that
/// is the whole point of it being a chassis. It answers `None` for a place
/// inside the body that belongs to nothing in particular, which is the body
/// itself.
#[must_use]
pub fn float_hit(
    geometry: &FloatGeometry,
    x: f32,
    y: f32,
    body: impl Fn(f32, f32) -> Option<FloatPart>,
) -> Option<FloatPart> {
    let hit = |rect: [f32; 4]| x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3];
    if !hit(geometry.frame) {
        return None;
    }
    // Before the head, because the grip overlaps nothing but is the smallest
    // thing here and the order is "smallest first" everywhere in this codebase.
    if geometry.grip.is_some_and(hit) {
        return Some(FloatPart::Grip);
    }
    if geometry.dock.is_some_and(hit) {
        return Some(FloatPart::Dock);
    }
    if geometry.flip.is_some_and(hit) {
        return Some(FloatPart::Flip);
    }
    if geometry.save.is_some_and(hit) {
        return Some(FloatPart::Save);
    }
    if hit(geometry.close) {
        return Some(FloatPart::Close);
    }
    if hit(geometry.head) {
        return Some(FloatPart::Head);
    }
    if hit(geometry.foot) {
        return Some(FloatPart::Foot);
    }
    if hit(geometry.body) {
        return Some(body(x - geometry.body[0], y - geometry.body[1]).unwrap_or(FloatPart::Body));
    }
    Some(FloatPart::Head)
}

// ── the entrance, and its reverse ───────────────────────────────────────────

/// How far through its entrance (or exit) a float is: opacity, and how far it
/// still has to rise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatFade {
    /// `0.0 ..= 1.0`, the layer's own opacity.
    pub opacity: f32,
    /// Physical pixels the frame is still displaced *upward* by — `flyIn`'s
    /// `translateY(-5px)` on its way to `none`.
    pub rise: f32,
    /// Whether another frame is owed.
    pub moving: bool,
}

/// Sample `flyIn` (or, when `reverse`, `flyOut`) at `elapsed`.
fn fade(elapsed: Duration, reverse: bool, motion: Motion, scale: f32) -> FloatFade {
    // `@media (prefers-reduced-motion: reduce) { #files-flyout { animation:
    // none } }` — the mock-up turns both keyframes off outright, so there is no
    // curve to sample and the window is simply there or gone.
    if motion == Motion::Reduced {
        return FloatFade {
            opacity: if reverse { 0.0 } else { 1.0 },
            rise: 0.0,
            moving: false,
        };
    }
    let progress = (elapsed.as_secs_f32() / FLOAT_ANIMATION.as_secs_f32()).clamp(0.0, 1.0);
    let eased = crate::cubic_bezier(progress, crate::EASE);
    let forward = if reverse { 1.0 - eased } else { eased };
    FloatFade {
        opacity: forward,
        rise: (1.0 - forward) * FLOAT_WINDOW_RISE_LOGICAL_PX * scale,
        moving: progress < 1.0,
    }
}

// ── the host ────────────────────────────────────────────────────────────────

/// A window's identity, for the whole of its life.
///
/// The same number the worker's traffic is tagged with — see [`FloatWin::epoch`]
/// — because "which view asked for this directory" and "which window is being
/// dragged" are the same question asked by two subsystems, and minting a second
/// identifier for the second of them is how a window ends up being two things
/// that can disagree. It survives promotion: a peek that becomes pinned is the
/// same window, so a read in flight when the hand kept it still lands.
pub type FloatId = u64;

/// Every floating window this window has open, and the two clocks around the
/// transient one.
///
/// # Two stores, because there are two kinds
///
/// **Overturns §7.1.2「全窗口单例」 (user ruling 2026-08-12).** The singleton was
/// not merely a limit on how many trees you could see: paired with the (correct)
/// law that a hover may never hijack a pinned window, it meant that *one* pinned
/// float switched hovering off for the whole window — every other trigger went
/// dead, and the peek that a hover is for could never be summoned again. The
/// ruling is to fix that at the root rather than to carve an exception: a float
/// is a place, and places do not exclude one another.
///
/// So the windows live in two stores, and the split is the two promises of
/// §7.1.2 rather than a convenience:
///
/// * [`Self::pinned`] is a **list, and the list is the z-order** — the tail is
///   the topmost. A pinned window was asked for, so it stays until an explicit
///   verb takes it away, and "which of these two is in front" is a real question
///   that only an order can answer.
/// * [`Self::peek`] is an **option, because a peek is singular by nature** and
///   not by ruling: it is the answer to "where is the pointer resting", there is
///   one pointer, and a second peek would be an answer to a question nobody
///   asked. It is always drawn on top — it is the newest thing on screen and the
///   most temporary, and a transient window buried under a permanent one is a
///   window you cannot see and cannot get rid of.
///
/// **The list admits duplicates by root, and that is deliberate** (2026-08-12,
/// the second of the day's two rulings — see [`Self::observe`]). Nothing here
/// asks whether a root is already open, because two windows on one folder are
/// two viewports rather than two copies: each has its own unfolding, its own
/// selection and its own scroll, which is what a person is asking for when they
/// open the same folder twice. Explorer and Finder both answer that way.
///
/// This is also the ground the preview float will stand on (§7.1's 小窗 is one
/// form with many tenants): the second floating tenant does not need a second
/// host, it needs a second entry in this list.
#[derive(Debug, Default)]
pub struct FloatHost {
    /// A trigger the pointer is resting on, and when its peek comes due.
    settling: Option<(FloatTrigger, Instant)>,
    /// The pinned windows, **bottom to top** — the tail is the frontmost.
    ///
    /// Dismissed ones stay here until [`Self::sweep`] retires them, so their
    /// exit can be seen; they answer nothing in the meantime, which is
    /// [`FloatWin::is_live`]'s whole job.
    pinned: Vec<FloatWin>,
    /// The transient peek, live or playing its exit.
    peek: Option<FloatWin>,
    /// When a transient peek's grace runs out.
    ///
    /// **Time, not space** (G84): the peek is not held open by a dead zone the
    /// pointer must stay inside, it is dismissed by a clock that the pointer
    /// keeps resetting while it is near. And there is deliberately no "focus is
    /// inside, so keep it" guard — a transient peek is never keyboard-driven, so
    /// the only way focus lands in one is an incidental click on a row, and that
    /// must not wedge it open after the pointer has gone. A peek you want to
    /// keep is one you pin.
    ///
    /// One clock rather than one per window, because only the peek is ever on
    /// it: a pinned window has no grace to run out.
    closing_at: Option<Instant>,
    /// Bumped on every open, so every window carries a number no other window
    /// has ever carried — see [`FloatId`].
    epoch: FloatId,
}

/// A file tree inside a float — the chassis's first tenant.
#[derive(Debug, Default)]
pub struct FloatFiles {
    /// Its own throwaway `{root, open, sel}` (G81) — never a leaf's, never
    /// persisted.
    pub files: crate::seats::FilesLeafState,
    /// Its own directory cache, which is also where its scroll position lives —
    /// so `Dock`/pop-out carry it for free, which is the half of G97 the mock-up
    /// never managed.
    pub cache: crate::files::DirCache,
    /// **What it knows about the repository under that root** (user ruling,
    /// 2026-08-19).
    ///
    /// R2 used to say a float had no Git page at all — "the flyout was never
    /// given the switch" — and the paint acted on it by drawing the tree
    /// whatever page the view was on. Undocking a column that was *standing* on
    /// its Git page therefore dropped the reader back onto the file tree without
    /// saying so, and the ruling that answered the report is the plain one: a
    /// float is the same place seen the same way it was being seen, so it keeps
    /// the page it was torn off on and the page works.
    ///
    /// Its own cache and not a share of the column's, for `cache`'s reason
    /// exactly: the column it came out of no longer exists, and a pinned window
    /// outlives the tab it was born in. It travels through `Dock` and the
    /// pop-out in both directions, so neither move re-reads a repository that
    /// has already answered.
    pub git: crate::git::GitCache,
    /// How far the Git page is scrolled, in physical pixels.
    ///
    /// Beside `cache.scroll_px` rather than sharing it: the two pages are two
    /// lists of different lengths in the same rectangle, and one number for both
    /// would make turning the page jump the one you arrived at.
    pub git_scroll: f32,
    /// The column width this view carries between float and dock (F75/G97).
    pub width: bt_layout::LogicalPx,
}

/// A preview buffer inside a float — the chassis's **second** tenant (P43-P67).
///
/// Almost empty, and that is the design working. §7.1.3 puts buffer ownership on
/// the *tab*, so what the window is showing, how far it is scrolled and where its
/// caret is all live in that tab's content plane under
/// `PreviewSurface::Float(id)` — exactly where a docked preview pane's do. All
/// this window has to remember is **whose** pool to read, which is the mock-up's
/// own `pvFloatState = { wsId, buf }` with the buffer taken out because the pool
/// already holds it.
#[derive(Clone, Copy, Debug)]
pub struct FloatPreview {
    /// The tab whose pool and content plane this window draws from.
    pub tab: TabId,
}

/// What is living inside a float this time.
///
/// The module header promised this: "the files tree is the first thing shown
/// inside it, not the thing it is for". An enum rather than two optional fields,
/// because a window shows one thing — two `Option`s can both be `Some`, and the
/// frame that finds them both has no rule to fall back on.
/// **The tree is boxed and the buffer is not**, which is the size difference
/// stated rather than suffered: a files view carries two caches — a directory
/// listing and, since 2026-08-19, a repository — while a buffer view carries a
/// `TabId`, because §7.1.3 keeps the buffer itself in the tab's pool. An enum as
/// large as its largest arm would make every entry in the host's `Vec` pay for a
/// tenant it is not.
#[derive(Debug)]
pub enum FloatTenant {
    Files(Box<FloatFiles>),
    Preview(FloatPreview),
}

/// The float itself: what it is showing, where it is, and how it got there.
#[derive(Debug)]
pub struct FloatWin {
    pub mode: FloatMode,
    /// The header it can be re-clicked from, or `None` once torn off a column.
    pub origin: FloatOrigin,
    /// What is inside.
    pub tenant: FloatTenant,
    /// The window's box in physical pixels.
    pub frame: [f32; 4],
    /// Whether the tree inside holds the keyboard.
    ///
    /// Only ever true for a pinned window: a hover must not hijack the
    /// terminal's keys (G90), so a peek is drawn with no focus ring and answers
    /// no keystrokes at all.
    pub focused: bool,
    /// Which worker traffic belongs to this view.
    pub epoch: u64,
    /// The trigger's box at the moment this float was summoned, or `None` for one
    /// that was popped out of a column.
    ///
    /// Kept so the window can be *re-placed* while it is still sizing itself to
    /// its content — see [`Self::self_sizing`].
    pub anchor: Option<[f32; 4]>,
    /// Whether this window is still sizing itself to what is inside it.
    ///
    /// **`height: auto` with a `max-height` is not a size taken once.** A browser
    /// grows the box as its content arrives and stops at the cap; the mock-up
    /// never had to think about it because its tree is a literal in the same
    /// file, available in the frame the window opens. Here the root is read on a
    /// worker, so a float measured only at birth is measured when it holds
    /// nothing — it opens as a bare strip and stays one however many rows arrive
    /// a moment later. Real-machine capture caught exactly that.
    ///
    /// It goes false the first time a hand moves or resizes the window, which is
    /// the other half of the CSS: an inline `width`/`height` from a drag beats
    /// `height: auto`, and from then on the size is the user's answer and not the
    /// content's.
    pub self_sizing: bool,
    /// When the entrance began.
    opened_at: Instant,
    /// When the exit began, if it has.
    dismissed_at: Option<Instant>,
}

impl FloatWin {
    /// Where this float is in its entrance or exit.
    #[must_use]
    pub fn fade(&self, now: Instant, motion: Motion, scale: f32) -> FloatFade {
        match self.dismissed_at {
            Some(at) => fade(now.saturating_duration_since(at), true, motion, scale),
            None => fade(
                now.saturating_duration_since(self.opened_at),
                false,
                motion,
                scale,
            ),
        }
    }

    /// Whether this float still answers the pointer and the keyboard.
    ///
    /// A dismissed one does not, and that is `pointer-events: none` on
    /// `.closing`: it is on screen only because taking a window away in one frame
    /// looks like a fault, and something you cannot click is not a window any
    /// more — it is the memory of one.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.dismissed_at.is_none()
    }

    /// The tree inside, if that is what is inside.
    #[must_use]
    pub fn files(&self) -> Option<&FloatFiles> {
        match &self.tenant {
            FloatTenant::Files(files) => Some(files),
            FloatTenant::Preview(_) => None,
        }
    }

    /// The same, mutably.
    pub fn files_mut(&mut self) -> Option<&mut FloatFiles> {
        match &mut self.tenant {
            FloatTenant::Files(files) => Some(files),
            FloatTenant::Preview(_) => None,
        }
    }

    /// The buffer inside, if that is what is inside.
    #[must_use]
    pub fn preview(&self) -> Option<FloatPreview> {
        match &self.tenant {
            FloatTenant::Files(_) => None,
            FloatTenant::Preview(preview) => Some(*preview),
        }
    }
}

impl FloatHost {
    // ── reading the two stores ──────────────────────────────────────────────

    /// Every window that should be drawn, **bottom to top** — dismissed ones
    /// included, because an exit has to be seen.
    ///
    /// This iterator *is* the z-order, and it is the only place that order is
    /// stated: the pinned list in its own order, then the peek over all of them.
    pub fn drawn(&self) -> impl Iterator<Item = &FloatWin> {
        self.pinned.iter().chain(self.peek.iter())
    }

    /// Every window that answers the pointer, **top to bottom** — the order a
    /// hit test must ask them in, which is the reverse of the one they are
    /// painted in.
    pub fn hit_order(&self) -> impl Iterator<Item = &FloatWin> {
        self.peek
            .iter()
            .chain(self.pinned.iter().rev())
            .filter(|win| win.is_live())
    }

    /// Every live window, in paint order.
    pub fn live_windows(&self) -> impl Iterator<Item = &FloatWin> {
        self.drawn().filter(|win| win.is_live())
    }

    /// The same, mutably.
    pub fn live_windows_mut(&mut self) -> impl Iterator<Item = &mut FloatWin> {
        self.pinned
            .iter_mut()
            .chain(self.peek.iter_mut())
            .filter(|win| win.is_live())
    }

    /// One live window by identity.
    #[must_use]
    pub fn live(&self, id: FloatId) -> Option<&FloatWin> {
        self.live_windows().find(|win| win.epoch == id)
    }

    /// The same, mutably — the worker's answer comes home through this.
    pub fn live_mut(&mut self, id: FloatId) -> Option<&mut FloatWin> {
        self.live_windows_mut().find(|win| win.epoch == id)
    }

    /// The live peek, if there is one.
    #[must_use]
    pub fn peek(&self) -> Option<&FloatWin> {
        self.peek.as_ref().filter(|win| win.is_live())
    }

    /// The live peek's identity.
    #[must_use]
    pub fn peek_id(&self) -> Option<FloatId> {
        self.peek().map(|win| win.epoch)
    }

    /// The frontmost live window — the peek if one is up, otherwise the top of
    /// the pinned list. What Esc is aimed at.
    #[must_use]
    pub fn top(&self) -> Option<&FloatWin> {
        self.hit_order().next()
    }

    /// Whether `id` names a live *pinned* window.
    ///
    /// The question every gesture asks before it starts: a peek has no grip, its
    /// header is not a handle, and its window is not yours to carry — yet.
    #[must_use]
    pub fn is_pinned(&self, id: FloatId) -> bool {
        self.live(id).is_some_and(|win| !win.mode.is_transient())
    }

    /// Bring a pinned window to the front. Returns whether the order changed.
    ///
    /// A press anywhere inside a window raises it, which is what every stacking
    /// window manager does and the only thing that makes a buried window
    /// reachable. The return value is a **frame debt**: a raise that redraws
    /// nothing is a click that visibly did nothing.
    ///
    /// The peek is not raisable because it is never anywhere else.
    pub fn raise(&mut self, id: FloatId) -> bool {
        let Some(index) = self
            .pinned
            .iter()
            .position(|win| win.epoch == id && win.is_live())
        else {
            return false;
        };
        self.focus_only(id);
        if index + 1 == self.pinned.len() {
            return false;
        }
        let win = self.pinned.remove(index);
        self.pinned.push(win);
        true
    }

    /// Hand the keyboard bit to one window and take it from the others.
    ///
    /// Nothing reads it yet — a float's tree answers no keys (see `float_layer`'s
    /// `focus_ring: false`) — but there is one keyboard, and a field that said
    /// three windows had it would be a lie waiting for the reader that arrives.
    fn focus_only(&mut self, id: FloatId) {
        for win in self.pinned.iter_mut().chain(self.peek.iter_mut()) {
            win.focused = win.epoch == id && win.mode == FloatMode::Pinned;
        }
    }

    /// Whether a *transient* peek is up — **the dismissal grace's own question,
    /// and only that.**
    ///
    /// [`Self::release`] has nothing to arm a grace for without one, and
    /// [`Self::grace_expired`] has nothing to take away. A pinned window is torn
    /// off and free-standing and no clock of this kind runs for it, which is what
    /// the peek slot already says: it holds the transient window and never a
    /// pinned one.
    ///
    /// **Not `railBusy`'s clause, and it used to be misread as one** (2026-08-15).
    /// A rail is held open by a peek *hanging off one of its own rows*, which is
    /// a question about the peek's trigger and not about its existence; asked as
    /// a bare "is a peek up" it made the folder button on every terminal pane's
    /// head a second, invisible rail trigger. That question lives with the rail
    /// now — see `rail_zone_wants_open` in `main.rs`, which reads [`Self::peek`]
    /// and asks its [`FloatWin::origin`].
    #[must_use]
    pub fn peek_is_open(&self) -> bool {
        self.peek().is_some()
    }

    // ── the two clocks ──────────────────────────────────────────────────────

    /// Note the trigger under the pointer and arm the intent (G86/H112).
    ///
    /// **The one guard is the peek's own.** Resting on the trigger whose peek is
    /// already up re-arms nothing, for the tooltip's reason: a trembling hand
    /// would otherwise close and reopen it forever. That is the whole of it — a
    /// *pinned* window blocks no hover at all, however it was summoned.
    ///
    /// # Two rulings, in the order they were made
    ///
    /// **改判 2026-08-12 (i) — hover is no longer switched off by a pinned
    /// window.** This used to begin `if self.pinned_is_open() { … return }`,
    /// which read as the 「hover 永不劫持 pinned」 law but was a much larger
    /// claim: one pinned window anywhere made *every* trigger in the product stop
    /// answering a hover, so the peek could never be summoned again until it was
    /// closed (user report). The law itself is untouched and in fact stronger now
    /// — a hover cannot move, replace or close a pinned window, because a peek is
    /// a window of its own in a slot of its own and never touches the pinned
    /// list. See [`FloatHost`]'s own note.
    ///
    /// **改判 2026-08-12 (ii) — 同根去重 repealed, and the guard narrowed with
    /// it.** The morning's fix replaced `pinned_is_open` with "any live window
    /// was summoned from this trigger", which was still too wide: a trigger whose
    /// window you had kept went quiet, so the hover worked everywhere except the
    /// places you had already been. Same disease, smaller blast radius. The guard
    /// now asks only about the peek, which is the only window a *re-arm* could
    /// disturb. Opening a second window on a root you already have is legal and
    /// ordinary — see
    /// `a_second_ask_for_a_root_already_open_is_answered_like_any_other` for the
    /// four reasons.
    pub fn observe(&mut self, trigger: Option<FloatTrigger>, now: Instant) {
        match trigger {
            Some(trigger) => {
                if self.peek().is_some_and(|win| win.origin == Some(trigger)) {
                    self.settling = None;
                    return;
                }
                if self.settling.map(|(armed, _)| armed) != Some(trigger) {
                    self.settling = Some((trigger, now + FLY_OPEN));
                }
            }
            None => self.settling = None,
        }
    }

    /// The trigger whose intent has matured, if one has.
    pub fn take_due(&mut self, now: Instant) -> Option<FloatTrigger> {
        let (trigger, due) = self.settling?;
        if now < due {
            return None;
        }
        self.settling = None;
        Some(trigger)
    }

    /// Throw away a pending intent (G87).
    ///
    /// Load-bearing on the Esc path: closing a peek while an intent was still
    /// maturing left the intent to mature 180ms later, under a pointer that had
    /// not moved — so the peek closed and then reopened by itself, and the only
    /// way out was to press Esc at exactly the right moment.
    pub fn disarm(&mut self) {
        self.settling = None;
    }

    /// Open a float.
    ///
    /// One entry point for all four ways a float comes to exist — hover, click,
    /// a click on another trigger, and a column popping out — because they differ
    /// only in the two arguments and having four would mean four places to forget
    /// the epoch.
    ///
    /// Where it lands is the mode's own answer: a peek takes the peek slot,
    /// replacing whatever was in it (there is one pointer, so there is one
    /// question); a pinned window is **appended** to the list and is therefore
    /// the frontmost, replacing nobody. That append is the whole of 浮窗多开.
    ///
    /// A pinned window opening also dismisses a live peek, and that is a
    /// judgement rather than an accident: the peek was the question this click is
    /// the answer to. Leaving it up would strand it — the pointer is on a trigger,
    /// which `drive_float_hover` counts as "still dealing with the peek", so its
    /// grace would never start and the question would sit over the answer.
    pub fn open(
        &mut self,
        mode: FloatMode,
        origin: FloatOrigin,
        tenant: FloatTenant,
        frame: [f32; 4],
        anchor: Option<[f32; 4]>,
        now: Instant,
    ) -> FloatId {
        self.settling = None;
        self.epoch += 1;
        let win = FloatWin {
            mode,
            origin,
            tenant,
            frame,
            focused: mode == FloatMode::Pinned,
            epoch: self.epoch,
            anchor,
            self_sizing: true,
            opened_at: now,
            dismissed_at: None,
        };
        match mode {
            FloatMode::Peek => {
                self.closing_at = None;
                self.peek = Some(win);
            }
            FloatMode::Pinned => {
                if let Some(peek) = self.peek.as_mut()
                    && peek.dismissed_at.is_none()
                {
                    peek.dismissed_at = Some(now);
                    self.closing_at = None;
                }
                self.pinned.push(win);
                self.focus_only(self.epoch);
            }
        }
        self.epoch
    }

    /// Promote the peek under this trigger into a pinned window, in place (G91).
    ///
    /// In place, and that is the point: the tree you were already looking at
    /// stays exactly as you had unfolded it, at the size and position it is
    /// already at. Reopening it as a new pinned window would reset both, and the
    /// gesture is called "keep this", not "start again".
    ///
    /// **Both clocks stop here.** A promoted window is not a peek any more, so
    /// neither of the two things that could take a peek away is allowed to
    /// survive the promotion: the dismissal grace, which would close a window the
    /// user is in the middle of carrying, and a *pending intent*, which would
    /// mature a moment later and reopen a second float over the one that had just
    /// been kept. `dismiss` and `wipe` null both for the same reason, and this is
    /// the third door out of peek-hood — it was the one that only nulled one of
    /// them (2026-08-12, when the header drag made promotion reachable with an
    /// intent still in flight).
    ///
    /// **It joins the pinned list, it does not replace it** (user ruling
    /// 2026-08-12): the window is appended, so a promoted peek stands in front of
    /// every window that was already there and takes none of them away. And it
    /// keeps its [`FloatId`] across the move, which is what lets a directory read
    /// that was in flight when the hand kept the window still find its way home.
    ///
    /// A sister already showing the same root is **not** consulted, merged with
    /// or overwritten — see `two_windows_on_one_root_unfold_independently`.
    ///
    /// Returns the promoted window's identity, so the gesture that promoted it
    /// can go on carrying *that* window and not merely "the float".
    pub fn promote(&mut self) -> Option<FloatId> {
        let mut win = self.peek.take().filter(FloatWin::is_live)?;
        win.mode = FloatMode::Pinned;
        let id = win.epoch;
        self.pinned.push(win);
        self.focus_only(id);
        self.closing_at = None;
        self.disarm();
        Some(id)
    }

    /// Begin one window's exit (§7.1.2's four closers, and nothing else).
    ///
    /// Returns whether there was anything to close. The window stays in hand
    /// until [`Self::sweep`] retires it, so the exit can be seen; the state it
    /// was showing is gone from that instant, which is what makes a closing float
    /// non-interactive without a second flag.
    ///
    /// A *peek*'s dismissal takes the pending intent with it (G87) and a pinned
    /// window's does not: the intent belongs to the hover machinery, and an Esc
    /// aimed at a window somebody tore off has no business cancelling a hover
    /// that is maturing over somewhere else entirely.
    pub fn dismiss(&mut self, id: FloatId, now: Instant) -> bool {
        if self.peek.as_ref().is_some_and(|win| win.epoch == id) {
            self.disarm();
            self.closing_at = None;
            let win = self.peek.as_mut().expect("just matched");
            if win.dismissed_at.is_some() {
                return false;
            }
            win.dismissed_at = Some(now);
            return true;
        }
        let Some(win) = self.pinned.iter_mut().find(|win| win.epoch == id) else {
            return false;
        };
        if win.dismissed_at.is_some() {
            return false;
        }
        win.dismissed_at = Some(now);
        true
    }

    /// Take one window away outright, without an exit — for the paths where the
    /// thing it was standing on has gone and there is nothing to play the
    /// animation against.
    pub fn wipe(&mut self, id: FloatId) -> Option<FloatWin> {
        if self.peek.as_ref().is_some_and(|win| win.epoch == id) {
            self.disarm();
            self.closing_at = None;
            return self.peek.take();
        }
        let index = self.pinned.iter().position(|win| win.epoch == id)?;
        Some(self.pinned.remove(index))
    }

    /// Take the peek away outright, whatever it is — the geometry change's own
    /// closer (§3.2: TRANSIENT dissolves).
    ///
    /// The two clocks are stopped **only when there was something to take**. This
    /// is now called on every viewport change rather than only when a peek was
    /// known to be up (a pinned window and a peek can be on screen together, so
    /// the caller no longer branches), and a resize that happened to land while a
    /// hover was maturing over an untouched trigger has no business cancelling it.
    pub fn wipe_peek(&mut self) -> Option<FloatWin> {
        let taken = self.peek.take()?;
        self.disarm();
        self.closing_at = None;
        Some(taken)
    }

    /// Retire every float whose exit has finished. Returns whether any was.
    pub fn sweep(&mut self, now: Instant, motion: Motion, scale: f32) -> bool {
        let done = |win: &FloatWin| !win.is_live() && !win.fade(now, motion, scale).moving;
        let before = self.pinned.len();
        self.pinned.retain(|win| !done(win));
        let mut swept = self.pinned.len() != before;
        if self.peek.as_ref().is_some_and(done) {
            self.peek = None;
            swept = true;
        }
        swept
    }

    /// The pointer is still dealing with the peek: cancel any pending dismissal.
    pub fn hold(&mut self) {
        self.closing_at = None;
    }

    /// The pointer has left: start the grace, unless one is already running.
    ///
    /// The "unless" is not an optimisation — re-arming on every pointer move
    /// would push the deadline away forever while the pointer drifted around
    /// outside, and the peek would never close at all.
    pub fn release(&mut self, off_left: bool, now: Instant) {
        if self.closing_at.is_some() || !self.peek_is_open() {
            return;
        }
        self.closing_at = Some(now + if off_left { FLY_CLOSE_LEFT } else { FLY_CLOSE });
    }

    /// Whether the grace has run out. Clears the clock either way it is read —
    /// **null first, then decide**, which is G85's bug written as a signature: a
    /// spent timer that still looked pending meant the *second* peek of a session
    /// never got a dismissal armed at all, and stayed up until Esc.
    pub fn grace_expired(&mut self, now: Instant) -> bool {
        let Some(at) = self.closing_at else {
            return false;
        };
        if now < at {
            return false;
        }
        self.closing_at = None;
        self.peek_is_open()
    }

    /// The next instant this host has something to do.
    #[must_use]
    pub fn deadline(
        &self,
        now: Instant,
        motion: Motion,
        scale: f32,
        frame: Duration,
    ) -> Option<Instant> {
        let animating = self
            .drawn()
            .any(|win| win.fade(now, motion, scale).moving)
            .then(|| now + frame);
        [
            self.settling.map(|(_, due)| due),
            self.closing_at,
            animating,
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

// ── drawing ─────────────────────────────────────────────────────────────────

/// The tenant's own contribution: whatever it draws inside [`FloatGeometry::body`].
///
/// `Default` is "nothing here", which is a real answer and not an absence: a
/// window holding a **picture** draws its body on the layer's image channel and
/// contributes no marks at all.
#[derive(Default)]
pub struct FloatBody {
    pub quads: Vec<OverlayQuad>,
    pub labels: Vec<ChromeLabel>,
    pub sprites: Vec<ChromeSprite>,
}

/// Everything about a float that is the **tenant's** answer rather than the
/// chassis's.
///
/// A struct rather than eleven positional arguments, and the reason is P43's
/// total: the chassis is shared, so the differences between two tenants have to
/// be *legible in one place* — this is that place, and every field of it is a
/// line of P49's difference table.
#[derive(Clone, Copy)]
pub struct FloatChrome<'a> {
    pub mode: FloatMode,
    /// The head's own icon: `#i-folder` for a tree, `#i-file` for a buffer.
    pub mark: ChromeMark,
    /// What the head says. Upper-cased **by the caller** — the files head shouts
    /// its root in caps and a filename keeps its case (P51), and a
    /// `text-transform` living in the chassis would apply that rule to both.
    pub title: &'a str,
    /// What the foot says, in full. That division is the mock-up's own note at
    /// line 731: the header names the leaf, the foot says where you are.
    ///
    /// Already cut to the room [`Self::notice`] left it — see
    /// [`crate::seats::dress_foot`], which every foot in this window goes
    /// through.
    pub path: &'a str,
    /// The standing fact hung on the foot's **right hand** — "Read-only ·
    /// 64 KB" — or empty (user ruling, 2026-08-15). A tree never has one.
    pub notice: &'a str,
    /// How wide that phrase draws, measured beside the renderer by the caller:
    /// this module holds no font.
    pub notice_width: f32,
    pub dock_label: &'a str,
    /// Side-honest (P54): the filled panel sits where the pane will land —
    /// [`ChromeMark::DockLeft`] for a tree, [`ChromeMark::DockRight`] for a
    /// preview.
    pub dock_mark: ChromeMark,
    pub hover: Option<FloatPart>,
    /// Whether the foot is confirming a reveal rather than showing the path
    /// (B24). The caption arrives from the caller, so this module holds no clock
    /// and no wording.
    pub revealed: bool,
    /// Whether the reserved dot is **occupied** (P16/P47). Meaningless without
    /// [`FloatHeadTools::dirty`], which is what reserved the slot.
    pub dirty: bool,
    /// Which way the flip is pointing: the glyph names the *destination*, so
    /// `#i-code` ("edit source") when the render is showing and `#i-eye` when the
    /// source is.
    pub flip_to_source: bool,
}

/// Draw the chassis around a body someone else filled in.
#[must_use]
pub fn build(
    geometry: &FloatGeometry,
    chrome: &FloatChrome<'_>,
    body: FloatBody,
    scale: f32,
    palette: &ChromePalette,
    fade: FloatFade,
) -> OverlayLayer {
    let FloatChrome {
        mode,
        mark,
        title: root_name,
        path,
        notice,
        notice_width,
        dock_label,
        dock_mark,
        hover,
        revealed,
        dirty,
        flip_to_source,
    } = *chrome;
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut quads: Vec<OverlayQuad> = Vec::new();
    let (inner_alpha, outer_alpha) = match mode {
        FloatMode::Peek => (
            palette.float_shadow_inner_alpha,
            palette.float_shadow_outer_alpha,
        ),
        FloatMode::Pinned => (
            palette.float_pinned_shadow_inner_alpha,
            palette.float_pinned_shadow_outer_alpha,
        ),
    };
    let border = px(FLOAT_WINDOW_BORDER_LOGICAL_PX).max(1.0).round();
    crate::settings::push_float_window(
        &mut quads,
        geometry.frame,
        px(FLOAT_WINDOW_RADIUS_LOGICAL_PX),
        border,
        px(FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.dialog_surface,
        palette.menu_shadow,
        alpha(inner_alpha),
        alpha(outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );
    // The two hairlines. `--border-soft` is not a palette entry of its own; the
    // pane head's edge is the same weight of separation over the same kind of
    // surface, and reusing it keeps one answer to "how loud is a rule inside a
    // panel" rather than minting a second.
    for edge in [geometry.head_edge, geometry.foot_edge] {
        quads.push(OverlayQuad {
            rect: edge,
            color: palette.pane_head_edge,
            alpha: 1.0,
        });
    }
    if hover == Some(FloatPart::Foot) {
        // `.fly-foot:hover { background: var(--hover) }`, stopped at the face's
        // own corners: the strip is the bottom of a rounded window, and a square
        // fill painted over the curve (user-reported 2026-08-12). The bottom
        // corners take the face's radius — the frame's less the border it sits
        // inside — and the top edge, which is interior, is squared back off by
        // a plain band laid over the rounded fill's upper reach.
        let radius = (px(FLOAT_WINDOW_RADIUS_LOGICAL_PX) - border).max(0.0);
        quads.extend(bt_render::rounded_overlay_fill(
            geometry.foot,
            radius,
            palette.dialog_hover,
            1.0,
        ));
        quads.push(OverlayQuad {
            rect: [
                geometry.foot[0],
                geometry.foot[1],
                geometry.foot[2],
                (geometry.foot[1] + radius).min(geometry.foot[3]),
            ],
            color: palette.dialog_hover,
            alpha: 1.0,
        });
    }
    let mut sprites = Vec::new();
    let mut labels = Vec::new();
    sprites.push(ChromeSprite::new(mark, geometry.head_mark, palette.accent));
    labels.push(ChromeLabel {
        text: root_name.to_owned(),
        rect: geometry.head_title,
        font_size_px: px(FLOAT_HEAD_FONT_LOGICAL_PX),
        color: palette.dialog_muted_text,
        align_right: false,
        align_center: false,
        letter_spacing_em: FLOAT_HEAD_TRACKING_EM,
        weight: ChromeLabelWeight::SemiBold,
        tabular_numerals: false,
        clip: None,
    });
    // `.float-win .fly-head button { border-radius: 5px }` — the buttons' hover
    // fill is a rounded pill and not a rectangle, which is what a `ChromeMark`
    // gives and a quad cannot.
    let button_radius = px(FLOAT_BUTTON_RADIUS_LOGICAL_PX).round().max(1.0) as u32;
    if let Some(dock) = geometry.dock {
        let lit = hover == Some(FloatPart::Dock);
        if lit {
            sprites.push(ChromeSprite::new(
                ChromeMark::ControlPill {
                    radius_px: button_radius,
                },
                dock,
                palette.dialog_hover,
            ));
        }
        let glyph = px(FLOAT_DOCK_GLYPH_LOGICAL_PX);
        let glyph_left = dock[0] + px(FLOAT_DOCK_PADDING_X_LOGICAL_PX);
        let glyph_top = dock[1] + ((dock[3] - dock[1]) - glyph) / 2.0;
        sprites.push(ChromeSprite::new(
            dock_mark,
            snap([glyph_left, glyph_top, glyph_left + glyph, glyph_top + glyph]),
            if lit {
                palette.dialog_title_text
            } else {
                palette.dialog_muted_text
            },
        ));
        labels.push(ChromeLabel {
            text: dock_label.to_owned(),
            rect: [
                glyph_left + glyph + px(FLOAT_DOCK_GAP_LOGICAL_PX),
                dock[1],
                dock[2] - px(FLOAT_DOCK_PADDING_X_LOGICAL_PX),
                dock[3],
            ],
            font_size_px: px(FLOAT_DOCK_FONT_LOGICAL_PX),
            color: if lit {
                palette.dialog_title_text
            } else {
                palette.dialog_muted_text
            },
            align_right: false,
            align_center: false,
            letter_spacing_em: FLOAT_HEAD_TRACKING_EM,
            weight: ChromeLabelWeight::SemiBold,
            tabular_numerals: false,
            clip: None,
        });
    }
    // The dot, in its reserved slot. Drawn only when it is *on*: the slot is
    // reserved by the geometry either way, which is what makes it "appearing
    // shoves nothing" (P16) rather than "appearing is announced by a shove".
    //
    // The same glyph, colour and centring the docked head uses (P47 is the pane's
    // rule read on the other surface): one dot means one thing, and a second
    // drawing of it is a second thing to keep in step.
    if let Some(slot) = geometry.dirty
        && dirty
    {
        labels.push(ChromeLabel {
            text: crate::seats::PREVIEW_DIRTY_DOT.to_owned(),
            rect: slot,
            font_size_px: px(FLOAT_DIRTY_GLYPH_LOGICAL_PX),
            color: palette.accent,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
    for (box_, part, glyph_mark) in [
        (geometry.save, FloatPart::Save, ChromeMark::Save),
        (
            geometry.flip,
            FloatPart::Flip,
            if flip_to_source {
                ChromeMark::Code
            } else {
                ChromeMark::Eye
            },
        ),
    ] {
        let Some(box_) = box_ else {
            continue;
        };
        let lit = hover == Some(part);
        if lit {
            sprites.push(ChromeSprite::new(
                ChromeMark::ControlPill {
                    radius_px: button_radius,
                },
                box_,
                palette.dialog_hover,
            ));
        }
        let glyph = px(FLOAT_DOCK_GLYPH_LOGICAL_PX);
        let left = box_[0] + ((box_[2] - box_[0]) - glyph) / 2.0;
        let top = box_[1] + ((box_[3] - box_[1]) - glyph) / 2.0;
        sprites.push(ChromeSprite::new(
            glyph_mark,
            snap([left, top, left + glyph, top + glyph]),
            if lit {
                palette.dialog_title_text
            } else {
                palette.dialog_muted_text
            },
        ));
    }
    let close_lit = hover == Some(FloatPart::Close);
    if close_lit {
        sprites.push(ChromeSprite::new(
            ChromeMark::ControlPill {
                radius_px: button_radius,
            },
            geometry.close,
            palette.dialog_hover,
        ));
    }
    let close_glyph = px(FLOAT_CLOSE_GLYPH_LOGICAL_PX);
    let close_left =
        geometry.close[0] + ((geometry.close[2] - geometry.close[0]) - close_glyph) / 2.0;
    let close_top =
        geometry.close[1] + ((geometry.close[3] - geometry.close[1]) - close_glyph) / 2.0;
    sprites.push(ChromeSprite::new(
        ChromeMark::PaneClose,
        snap([
            close_left,
            close_top,
            close_left + close_glyph,
            close_top + close_glyph,
        ]),
        if close_lit {
            palette.dialog_title_text
        } else {
            palette.dialog_muted_text
        },
    ));

    quads.extend(body.quads);
    labels.extend(body.labels);
    sprites.extend(body.sprites);

    // `.fly-foot.done { color: var(--accent) }` — the confirmation takes both the
    // mark and the words, because a tick beside an unchanged path would read as a
    // property of the folder rather than as an answer to the click.
    let foot_ink = if revealed {
        palette.accent
    } else if hover == Some(FloatPart::Foot) {
        palette.dialog_secondary_text
    } else {
        palette.dialog_muted_text
    };
    sprites.push(ChromeSprite::new(
        if revealed {
            ChromeMark::Check
        } else {
            ChromeMark::FolderOpen
        },
        geometry.foot_mark,
        foot_ink,
    ));
    // The right hand and what it costs the path — the docked foot's rule, in
    // the float's own numbers (user ruling, 2026-08-15).
    let (path_box, notice_box) = crate::seats::foot_notice_split(
        geometry.foot_path,
        notice_width,
        px(crate::seats::FILES_FOOT_NOTICE_GAP_LOGICAL_PX),
    );
    labels.push(ChromeLabel {
        text: path.to_owned(),
        rect: path_box,
        font_size_px: px(FLOAT_FOOT_FONT_LOGICAL_PX),
        color: foot_ink,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(path_box),
    });
    if !notice.is_empty() {
        labels.push(ChromeLabel {
            text: notice.to_owned(),
            rect: notice_box,
            font_size_px: px(FLOAT_FOOT_FONT_LOGICAL_PX),
            // The strip's palest ink whatever the hover is doing: this half is a
            // fact about the file, not part of the button's label.
            color: palette.dialog_muted_text,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(notice_box),
        });
    }
    if let Some(grip) = geometry.grip {
        let glyph = px(FLOAT_GRIP_GLYPH_LOGICAL_PX);
        let inset = px(FLOAT_GRIP_GLYPH_INSET_LOGICAL_PX);
        sprites.push(ChromeSprite::new(
            ChromeMark::ResizeGrip,
            snap([
                grip[2] - inset - glyph,
                grip[3] - inset - glyph,
                grip[2] - inset,
                grip[3] - inset,
            ]),
            palette.dialog_muted_text,
        ));
    }
    OverlayLayer {
        quads,
        labels,
        sprites,
        opacity: fade.opacity.clamp(0.0, 1.0),
        ..OverlayLayer::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE: f32 = 1.0;
    /// A window with room for anything the tests below ask for.
    const VIEWPORT: [f32; 4] = [0.0, 34.0, 1280.0, 800.0];

    fn frame(left: f32, top: f32, width: f32, height: f32) -> [f32; 4] {
        [left, top, left + width, top + height]
    }

    /// A tree tenant on this root, at the column's opening width.
    fn files_tenant(root: &str) -> FloatTenant {
        FloatTenant::Files(Box::new(FloatFiles {
            files: state(root),
            width: bt_layout::LogicalPx::px(240),
            ..FloatFiles::default()
        }))
    }

    /// G79: the chassis is the mock-up's, value for value — a 30px head, a 30px
    /// foot, and a body that is whatever is left between them.
    #[test]
    fn the_chassis_gives_its_head_and_foot_thirty_pixels_each_and_the_body_the_rest() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 264.0, 400.0),
            FloatMode::Peek,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        assert_eq!(
            geometry.head[3] - geometry.head[1],
            30.0,
            "the head's height"
        );
        assert_eq!(
            geometry.foot[3] - geometry.foot[1],
            30.0,
            "the foot's height"
        );
        assert_eq!(
            geometry.body[1], geometry.head_edge[3],
            "the body starts under the head's hairline"
        );
        assert_eq!(
            geometry.body[3], geometry.foot_edge[1],
            "and stops at the foot's"
        );
    }

    /// G79: only a pinned float wears the grip. A peek cannot be resized because
    /// a peek is not yours yet.
    #[test]
    fn only_a_pinned_float_wears_the_corner_grip() {
        let box_ = frame(100.0, 100.0, 264.0, 400.0);
        assert!(
            float_geometry(
                box_,
                FloatMode::Peek,
                SCALE,
                30.0,
                FloatHeadTools::default()
            )
            .grip
            .is_none(),
            "a transient peek has no grip"
        );
        let grip = float_geometry(
            box_,
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        )
        .grip
        .expect("a pinned float has one");
        assert_eq!(grip[2] - grip[0], 16.0, "16px wide");
        assert_eq!(grip[3] - grip[1], 16.0, "16px tall");
    }

    /// The head's trailing run is built right to left, `DOCK` before `×` — the
    /// pane head's own rule, one surface over.
    #[test]
    fn the_dock_button_sits_left_of_the_close_and_the_title_yields_to_both() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 264.0, 400.0),
            FloatMode::Peek,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        let dock = geometry.dock.expect("there is room for it");
        assert!(dock[2] < geometry.close[0], "DOCK stands left of the ×");
        assert!(
            geometry.head_title[2] <= dock[0],
            "and the title stops before DOCK"
        );
        assert!(
            geometry.head_title[0] >= geometry.head_mark[2],
            "starting after the folder mark"
        );
    }

    /// PIN — the `DOCK` box holds the whole of the caption it was sized for, at
    /// every scale and every fractional measured width, and still clears the `×`.
    ///
    /// Two numbers meet in this box and both are fractional: the caption's
    /// measured width, which the font decides, and `close[0] - gap`, which a
    /// fractional scale decides. The box used to be `snap`ped — each edge
    /// rounded to the nearer pixel independently — so half of those pairs lost
    /// up to a pixel off the *inside* of the button. The label's own rect is this
    /// box less its two paddings, so the pixel came straight out of the
    /// caption's room and the last letter was clipped by the bounds around it.
    ///
    /// Red gate: with `snap`, `dock_label_px = 22.6` at scale 1.0 gives a box of
    /// 45 device pixels where the caption needs 45.6 — the assertion below fails
    /// by 0.6, which is exactly the fraction of a `K` the user photographed.
    #[test]
    fn the_dock_box_is_never_rounded_narrower_than_its_own_caption() {
        for scale in [1.0_f32, 1.25, 1.5, 1.75, 2.0] {
            // The measured caption is whatever the face says; these stand in for
            // the fractions it produces, since this module has no font.
            for label_px in [18.0_f32, 22.6, 23.4, 30.0, 41.07] {
                let geometry = float_geometry(
                    frame(100.5, 100.0, 264.0 * scale, 400.0),
                    FloatMode::Pinned,
                    scale,
                    label_px,
                    FloatHeadTools::default(),
                );
                let dock = geometry.dock.expect("a 264px head seats the button");
                let wanted = (FLOAT_DOCK_PADDING_X_LOGICAL_PX * 2.0
                    + FLOAT_DOCK_GLYPH_LOGICAL_PX
                    + FLOAT_DOCK_GAP_LOGICAL_PX)
                    * scale
                    + label_px;
                assert!(
                    dock[2] - dock[0] >= wanted,
                    "scale {scale}, caption {label_px}: the box is {} against {wanted} of \
                     glyph, gap, padding and caption",
                    dock[2] - dock[0],
                );
                // The `×`'s own room is the other side of the same rounding, and
                // it is not allowed to pay for the button's.
                assert!(
                    dock[2] + FLOAT_HEAD_GAP_LOGICAL_PX * scale <= geometry.close[0],
                    "scale {scale}, caption {label_px}: DOCK's right edge {} plus its gap \
                     runs into the × at {}",
                    dock[2],
                    geometry.close[0],
                );
                // And both edges are still whole device pixels, which is what the
                // rounded pill fill behind the caption needs.
                assert_eq!(dock[0], dock[0].floor(), "the left edge is a whole pixel");
                assert_eq!(dock[2], dock[2].floor(), "and so is the right");
            }
        }
    }

    /// The `×` is the one control that may not be crowded out: it is the only one
    /// that can undo whatever made the window this narrow.
    #[test]
    fn a_head_with_no_room_drops_the_dock_button_and_keeps_the_close() {
        let geometry = float_geometry(
            frame(0.0, 0.0, 60.0, 200.0),
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        assert!(geometry.dock.is_none(), "DOCK gives up its box");
        assert!(
            geometry.close[2] > geometry.close[0],
            "the × keeps a real one"
        );
    }

    /// G89: under the trigger, left-aligned to it, six pixels down.
    #[test]
    fn a_fresh_float_opens_under_its_trigger_six_pixels_down() {
        let trigger = frame(300.0, 40.0, 19.0, 19.0);
        let placed = float_placement(trigger, [264.0, 400.0], VIEWPORT, SCALE);
        assert_eq!(
            placed,
            [300.0, 65.0, 564.0, 465.0],
            "left-aligned to the icon, 6px below, at the size it asked for"
        );
    }

    /// G89: no room below is answered by flipping *above*, which is
    /// `M2-tiny-window-priority.md` §3.3's 主轴翻转.
    #[test]
    fn a_float_with_no_room_below_flips_above_its_trigger() {
        let trigger = frame(300.0, 700.0, 19.0, 19.0);
        let placed = float_placement(trigger, [264.0, 400.0], VIEWPORT, SCALE);
        assert_eq!(
            placed[1],
            700.0 - 6.0 - 400.0,
            "above, with the same 6px gap"
        );
    }

    /// §3.3's other half: the cross axis is clamped and never flipped.
    #[test]
    fn a_float_near_the_right_edge_is_clamped_not_flipped() {
        let trigger = frame(1270.0, 40.0, 19.0, 19.0);
        let placed = float_placement(trigger, [264.0, 400.0], VIEWPORT, SCALE);
        assert_eq!(
            placed[0],
            1280.0 - 8.0 - 264.0,
            "pulled in to the 8px viewport margin"
        );
    }

    /// A window called from the **lower** pane of an up/down split hangs off
    /// *that* head, not off the top of the window (user report, 2026-08-12).
    ///
    /// The shape of the bug: at the DPI where the 62%-of-viewport cap outgrows
    /// the 460-logical-pixel one, a float is tall enough that **neither** side
    /// of a mid-window trigger can hold it — and the old placement answered
    /// that by flipping above, overflowing, and letting the cross-axis clamp
    /// drag it to the viewport's top edge. Left-aligned to a trigger that
    /// lives at the right end of a pane head, that lands in the window's
    /// top-right corner with nothing beneath it, which is exactly what the
    /// screenshot showed. Both triggers are asked here, from one real split,
    /// because the top pane's answer was never wrong and must stay right.
    #[test]
    fn a_float_summoned_from_the_lower_pane_head_hangs_off_that_head() {
        // The DPI is the point: at 1.5 the viewport fraction is the binding cap.
        const HIDPI: f32 = 1.5;
        let viewport = [0.0, 51.0, 1920.0, 1080.0];
        let split = [[0.0, 51.0, 1920.0, 565.0], [0.0, 565.0, 1920.0, 1080.0]];
        // Tall enough to want the whole cap — a home folder's worth of rows.
        let size = float_opening_size(100_000.0, viewport, HIDPI, FloatSizing::files());
        let gap = FLOAT_WINDOW_TRIGGER_GAP_LOGICAL_PX * HIDPI;
        for (which, pane) in split.iter().enumerate() {
            let trigger =
                crate::seats::pane_head_geometry(*pane, bt_layout::SeatKind::Terminal, HIDPI)
                    .files
                    .expect("a terminal head carries the folder trigger");
            let placed = float_placement(trigger, size, viewport, HIDPI);
            let below = (placed[1] - (trigger[3] + gap)).abs() <= 1.0;
            let above = ((trigger[1] - gap) - placed[3]).abs() <= 1.0;
            assert!(
                below || above,
                "pane {which}: the window hangs off its trigger {trigger:?}, \
                 not off the viewport — got {placed:?}"
            );
        }
    }

    /// §7.1.2: 主窗口缩小自动重钳位——平移回视口、尺寸不变.
    #[test]
    fn a_shrinking_window_translates_a_pinned_float_home_without_resizing_it() {
        let float = frame(900.0, 600.0, 264.0, 300.0);
        let clamped = clamp_pinned(float, [0.0, 34.0, 700.0, 500.0], SCALE);
        assert_eq!(clamped[2] - clamped[0], 264.0, "its width is untouched");
        assert_eq!(clamped[3] - clamped[1], 300.0, "and so is its height");
        assert_eq!(clamped[2], 700.0 - 6.0, "it has moved back inside");
    }

    /// §3.4's honest floor: squeezed past every limit it becomes a strip, and
    /// never nothing — the `×` and the drag handle have to survive, because they
    /// are the only two things that can undo this.
    #[test]
    fn a_pinned_float_squeezed_past_its_limits_stops_at_one_header_strip() {
        let float = frame(100.0, 100.0, 264.0, 300.0);
        let clamped = clamp_pinned(float, [0.0, 0.0, 120.0, 40.0], SCALE);
        assert_eq!(
            clamped[3] - clamped[1],
            FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX,
            "it stops at the header strip"
        );
        assert!(
            clamped[3] - clamped[1] > 0.0,
            "and a strip is not nothing — §7.1.2's four closers are still the only ones"
        );
    }

    /// The grip's floor is the design's 200×150, and it is a *different* floor
    /// from the squeeze's: a hand may not make the window useless, while a
    /// shrinking screen may make it a strip rather than take it away.
    #[test]
    fn the_grip_stops_at_two_hundred_by_one_fifty() {
        let float = frame(100.0, 100.0, 264.0, 400.0);
        let resized =
            float_resized_to(float, [110.0, 110.0], VIEWPORT, SCALE, FloatSizing::files());
        assert_eq!(
            resized[2] - resized[0],
            200.0,
            "the grip's own minimum width"
        );
        assert_eq!(resized[3] - resized[1], 150.0, "and its minimum height");
    }

    /// G83: the tolerance bridges the gap between the icon and the window, and
    /// reports which way a departure went so the caller can give a leftward one
    /// the long grace.
    #[test]
    fn the_peek_tolerance_reaches_eight_pixels_past_every_edge_and_names_the_left() {
        let float = frame(100.0, 100.0, 264.0, 300.0);
        assert!(
            peek_reach(float, 96.0, 200.0, SCALE).inside,
            "four pixels off the left edge is still dealing with it"
        );
        assert!(
            !peek_reach(float, 88.0, 200.0, SCALE).inside,
            "twelve is not"
        );
        assert!(
            peek_reach(float, 88.0, 200.0, SCALE).off_left,
            "and it left to the left"
        );
        assert!(
            !peek_reach(float, 500.0, 200.0, SCALE).off_left,
            "while a departure to the right is not a leftward one"
        );
    }

    /// Smallest target first, the same order the pane head and the tab strip use.
    #[test]
    fn the_hit_test_answers_the_controls_before_the_bar_they_sit_in() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 264.0, 400.0),
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        let centre = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        let (x, y) = centre(geometry.close);
        assert_eq!(
            float_hit(&geometry, x, y, |_, _| None),
            Some(FloatPart::Close)
        );
        let (x, y) = centre(geometry.dock.expect("present"));
        assert_eq!(
            float_hit(&geometry, x, y, |_, _| None),
            Some(FloatPart::Dock)
        );
        let (x, y) = centre(geometry.grip.expect("present"));
        assert_eq!(
            float_hit(&geometry, x, y, |_, _| None),
            Some(FloatPart::Grip)
        );
        assert_eq!(
            float_hit(
                &geometry,
                geometry.head[0] + 2.0,
                y_of(geometry.head),
                |_, _| None
            ),
            Some(FloatPart::Head),
            "bare header is the drag handle"
        );
        assert_eq!(
            float_hit(&geometry, 50.0, 50.0, |_, _| None),
            None,
            "outside"
        );
    }

    fn y_of(rect: [f32; 4]) -> f32 {
        (rect[1] + rect[3]) / 2.0
    }

    /// `.fly-foot:hover` lights the strip — but the strip is the bottom of a
    /// *rounded* window, and a full-alpha fill that reaches the corner pixel is
    /// the hover painted square over the face's curve (user-reported
    /// 2026-08-12). The corner may receive coverage, not opacity: only the
    /// anti-aliased partial quads of a rounded fill are allowed to touch it.
    #[test]
    fn the_foot_hover_never_paints_full_alpha_into_the_faces_rounded_corner() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 264.0, 400.0),
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        let palette = bt_render::chrome_palette();
        let layer = build(
            &geometry,
            &FloatChrome {
                mode: FloatMode::Pinned,
                mark: ChromeMark::Folder,
                title: "WEIYI",
                path: "C:\\Users\\Weiyi",
                notice: "",
                notice_width: 0.0,
                dock_label: "DOCK",
                dock_mark: ChromeMark::DockLeft,
                hover: Some(FloatPart::Foot),
                revealed: false,
                dirty: false,
                flip_to_source: false,
            },
            FloatBody {
                quads: Vec::new(),
                labels: Vec::new(),
                sprites: Vec::new(),
            },
            SCALE,
            &palette,
            FloatFade {
                opacity: 1.0,
                rise: 0.0,
                moving: false,
            },
        );
        let corner = [geometry.foot[0] + 0.5, geometry.foot[3] - 0.5];
        let offending: Vec<_> = layer
            .quads
            .iter()
            .filter(|quad| {
                quad.color == palette.dialog_hover
                    && quad.alpha >= 1.0
                    && quad.rect[0] <= corner[0]
                    && corner[0] <= quad.rect[2]
                    && quad.rect[1] <= corner[1]
                    && corner[1] <= quad.rect[3]
            })
            .collect();
        assert!(
            offending.is_empty(),
            "a full-alpha hover quad reached the face's corner: {offending:?}"
        );
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.dialog_hover),
            "the foot still lights on hover"
        );
    }

    /// The body hands its rows to the tenant's own hit test, in the body's
    /// coordinates — the chassis does not know what is inside it.
    /// **A window standing on its Git page draws one** (user ruling,
    /// 2026-08-19) — the report was a float with a head, a foot and nothing at
    /// all between them.
    ///
    /// The picture is the docked page's own painter handed this chassis's body
    /// rect, which is the whole of what the fix is: one page, two hosts, and
    /// nothing about `push_git_panel` that knows which one it is in. What this
    /// pins is the body it is given — a real chassis rectangle, not a strip —
    /// and that a repository that has answered fills it.
    ///
    /// Red gate: give the window the tree's painter whatever page the view is on
    /// (which is what it did) and this body carries the wrong list; give it a
    /// body of zero height (which is what the strip-sized window had) and it
    /// carries nothing.
    #[test]
    fn a_window_on_its_git_page_fills_its_body_with_the_repositorys_own_rows() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 264.0, 520.0),
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        assert!(
            geometry.body[3] - geometry.body[1] > 100.0,
            "the chassis leaves a body to draw into: {:?}",
            geometry.body
        );
        let content = crate::git_panel::sample_page_for_tests();
        assert!(!content.rows.is_empty(), "the fixture repository answered");
        let (mut quads, mut labels, mut sprites) = (Vec::new(), Vec::new(), Vec::new());
        crate::git_panel::push_git_panel(
            geometry.body,
            &content,
            crate::git_panel::GitHover::default(),
            SCALE,
            &bt_render::chrome_palette(),
            (&mut quads, &mut labels, &mut sprites),
        );
        assert!(
            !labels.is_empty() && !sprites.is_empty(),
            "a floating Git page is not a blank window: {} labels, {} sprites",
            labels.len(),
            sprites.len()
        );
        // And every mark of it is inside the body the chassis gave it — a window
        // is not entitled to paint over its own head or its own foot.
        for rect in labels
            .iter()
            .map(|label| label.rect)
            .chain(sprites.iter().map(|sprite| sprite.rect))
        {
            assert!(
                rect[0] >= geometry.body[0] - 0.5 && rect[2] <= geometry.body[2] + 0.5,
                "drawn inside the body: {rect:?} in {:?}",
                geometry.body
            );
        }
    }

    /// PIN (user report, 2026-08-20) — **a window showing the commit graph is
    /// not a blank window.**
    ///
    /// The twin of the Git page's pin above, and it is owed for a defect that
    /// was invisible for exactly the reason a graph is drawn the way it is: a
    /// graph's `PreviewDocument` is empty *on purpose*, because the picture is
    /// chrome pushed into the body rectangle. So a host that knew about the
    /// document pipeline and the picture channel and nothing else drew a head, a
    /// foot, and no third thing — and had nothing to report, because every
    /// machine it did know about had done its job.
    ///
    /// Red gate: hand the window's body to the document pipeline (which is what
    /// it did) and this rectangle carries nothing at all.
    #[test]
    fn a_window_showing_a_commit_graph_fills_its_body_with_the_repositorys_own_rows() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 520.0, 520.0),
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        let content = crate::git_graph::sample_graph_for_tests(geometry.body, SCALE);
        assert!(
            content.total_rows > 0,
            "the fixture repository answered with a history"
        );
        let (mut quads, mut labels, mut sprites) = (Vec::new(), Vec::new(), Vec::new());
        crate::git_graph::push_graph(
            geometry.body,
            &content,
            crate::git_graph::GraphHover::default(),
            SCALE,
            &bt_render::chrome_palette(),
            (&mut quads, &mut labels, &mut sprites),
        );
        assert!(
            !labels.is_empty() && !sprites.is_empty(),
            "a floating commit graph is not a blank window: {} labels, {} sprites",
            labels.len(),
            sprites.len()
        );
        // The masthead, the column header and the commit rows are all words, so
        // the subject of a commit the fixture wrote is on the glass.
        assert!(
            labels.iter().any(|label| label.text.contains("commit c3")),
            "the newest commit's subject is drawn: {:?}",
            labels.iter().map(|label| &label.text).collect::<Vec<_>>()
        );
        // And every mark of it is inside the body the chassis gave it — a window
        // is not entitled to paint over its own head or its own foot.
        for rect in labels
            .iter()
            .map(|label| label.rect)
            .chain(sprites.iter().map(|sprite| sprite.rect))
        {
            assert!(
                rect[0] >= geometry.body[0] - 0.5 && rect[2] <= geometry.body[2] + 0.5,
                "drawn inside the body: {rect:?} in {:?}",
                geometry.body
            );
        }
    }

    /// PIN (user report, 2026-08-20) — **the window asks the graph's own hit
    /// test, and gets the graph's own three answers.**
    ///
    /// What this pins is that the chassis has a vocabulary for them at all. The
    /// float's body used to answer `Row` or nothing, which is a tree's and a Git
    /// page's whole vocabulary; a graph has a toolbar above its list and
    /// pressable parts inside one of its rows, and a host with no word for those
    /// would have sent both to `press_graph_row` as if they were rows.
    #[test]
    fn a_window_names_the_graphs_toolbar_and_its_rows_apart() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 520.0, 520.0),
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        let content = crate::git_graph::sample_graph_for_tests(geometry.body, SCALE);
        let toolbar = content
            .toolbar
            .as_ref()
            .expect("a graph with a history draws its toolbar");
        let head = crate::git_graph::graph_geometry(geometry.body, &content, SCALE)
            .head
            .expect("the toolbar has a strip of its own above the list");
        let rects = crate::git_graph::graph_toolbar_rects(head, toolbar, SCALE);
        let refresh = [
            (rects.refresh[0] + rects.refresh[2]) / 2.0,
            (rects.refresh[1] + rects.refresh[3]) / 2.0,
        ];
        assert_eq!(
            float_hit(&geometry, refresh[0], refresh[1], |x, y| {
                let (x, y) = (x + geometry.body[0], y + geometry.body[1]);
                crate::git_graph::graph_hit(geometry.body, &content, SCALE, x, y).map(|hit| {
                    match hit {
                        crate::git_graph::GraphHit::Tool(tool) => FloatPart::GraphTool(tool),
                        crate::git_graph::GraphHit::Detail { index, part } => {
                            FloatPart::GraphDetail { index, part }
                        }
                        crate::git_graph::GraphHit::Row(index) => FloatPart::GraphRow(index),
                    }
                })
            }),
            Some(FloatPart::GraphTool(crate::git_graph::GraphTool::Refresh)),
            "the toolbar answers before the list, exactly as it does on a pane"
        );
    }

    #[test]
    fn the_body_asks_its_tenant_which_row_the_pointer_is_on() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 264.0, 400.0),
            FloatMode::Peek,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        let inside = (geometry.body[0] + 10.0, geometry.body[1] + 10.0);
        assert_eq!(
            float_hit(&geometry, inside.0, inside.1, |x, y| {
                assert!(x >= 0.0 && y >= 0.0, "coordinates arrive body-local");
                Some(FloatPart::Row(3))
            }),
            Some(FloatPart::Row(3))
        );
        assert_eq!(
            float_hit(&geometry, inside.0, inside.1, |_, _| None),
            Some(FloatPart::Body),
            "and a miss is the body itself"
        );
    }

    // ── the host ────────────────────────────────────────────────────────────

    fn state(root: &str) -> crate::seats::FilesLeafState {
        crate::seats::FilesLeafState {
            root: root.to_owned(),
            ..crate::seats::FilesLeafState::default()
        }
    }

    fn open_peek(host: &mut FloatHost, trigger: FloatTrigger, now: Instant) -> FloatId {
        open_peek_at(host, trigger, "C:/x", now)
    }

    fn open_peek_at(
        host: &mut FloatHost,
        trigger: FloatTrigger,
        root: &str,
        now: Instant,
    ) -> FloatId {
        host.open(
            FloatMode::Peek,
            Some(trigger),
            files_tenant(root),
            frame(100.0, 100.0, 264.0, 300.0),
            None,
            now,
        )
    }

    fn open_pinned_at(
        host: &mut FloatHost,
        trigger: Option<FloatTrigger>,
        root: &str,
        box_: [f32; 4],
        now: Instant,
    ) -> FloatId {
        host.open(
            FloatMode::Pinned,
            trigger,
            files_tenant(root),
            box_,
            None,
            now,
        )
    }

    /// The z-order as a list of identities, bottom to top.
    fn stack(host: &FloatHost) -> Vec<FloatId> {
        host.drawn().map(|win| win.epoch).collect()
    }

    const TAB_ID: TabId = TabId(1);
    const TAB: FloatTrigger = FloatTrigger::Tab(TAB_ID);
    const OTHER: FloatTrigger = FloatTrigger::Tab(TabId(2));
    const THIRD: FloatTrigger = FloatTrigger::Tab(TabId(3));

    /// G82: the intent takes 180ms, and it is not due before then.
    #[test]
    fn a_hover_becomes_a_peek_only_after_the_intent_matures() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        host.observe(Some(TAB), now);
        assert_eq!(host.take_due(now + FLY_OPEN / 2), None, "not yet");
        assert_eq!(host.take_due(now + FLY_OPEN), Some(TAB), "now");
        assert_eq!(host.take_due(now + FLY_OPEN), None, "and only once");
    }

    // ── 浮窗多开 (user ruling 2026-08-12) ────────────────────────────────────

    /// PIN ① — **a pinned window standing does not switch hover off.**
    ///
    /// The user's report, as arithmetic: with one pinned files window up,
    /// hovering any *other* folder trigger produced nothing at all, ever again.
    /// The cause was [`FloatHost::observe`]'s old opening line — `if
    /// self.pinned_is_open() { … return }` — which was written as the 「hover
    /// 永不劫持 pinned」 law but implemented a much larger one: hover was disabled
    /// window-wide rather than merely kept off the pinned window.
    ///
    /// Red gate / mutation: put that guard back at the top of `observe` and this
    /// returns `None` where it asks for `Some(OTHER)`.
    #[test]
    fn a_hover_still_arms_an_intent_while_a_pinned_window_stands() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        open_pinned_at(
            &mut host,
            Some(TAB),
            "C:/x",
            frame(100.0, 100.0, 264.0, 300.0),
            now,
        );
        host.observe(Some(OTHER), now);
        assert_eq!(
            host.take_due(now + FLY_OPEN),
            Some(OTHER),
            "another trigger's peek is still summonable"
        );
    }

    /// PIN ④ — **G86 restated and strengthened: a hover never touches a pinned
    /// window.**
    ///
    /// This test replaces `a_hover_never_hijacks_a_pinned_window`, whose
    /// assertion was "a pinned window makes hover do nothing" — which is the bug
    /// above, not the law. The law was never about disabling hover: it is that a
    /// passing pointer may not *move, replace or close* a window somebody asked
    /// for. That is what is asserted here, and the new shape makes it true by
    /// construction — a peek lives in its own slot and can no more reach into the
    /// pinned list than a pinned window can reach into the peek's.
    ///
    /// Red gate / mutation: let a peek's `open` push onto `self.pinned` instead
    /// of taking the peek slot, and the z-order and `peek_id` assertions fail.
    #[test]
    fn a_hover_never_moves_replaces_or_closes_a_pinned_window() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let boxes = [
            frame(100.0, 100.0, 264.0, 300.0),
            frame(500.0, 200.0, 264.0, 300.0),
        ];
        let first = open_pinned_at(&mut host, Some(TAB), "C:/x", boxes[0], now);
        let second = open_pinned_at(&mut host, Some(OTHER), "C:/y", boxes[1], now);

        // A hover matures over a third trigger and its peek arrives.
        host.observe(Some(THIRD), now);
        assert_eq!(host.take_due(now + FLY_OPEN), Some(THIRD));
        let peek = open_peek_at(&mut host, THIRD, "C:/z", now);

        assert_eq!(
            stack(&host),
            vec![first, second, peek],
            "the peek joined on top and displaced nobody"
        );
        for (id, box_) in [(first, boxes[0]), (second, boxes[1])] {
            let win = host.live(id).expect("still open");
            assert_eq!(win.frame, box_, "a hover moved nothing");
            assert_eq!(win.mode, FloatMode::Pinned, "and changed nobody's promise");
        }
        assert_eq!(host.peek_id(), Some(peek), "the peek is the peek");
    }

    /// PIN ② — **a promoted peek joins the list; it replaces nobody, and it
    /// arrives in front.**
    ///
    /// Rule ④ of the ruling. The window that was already there keeps its place
    /// in the world and loses only its position at the top, which is what
    /// "another window opened" means everywhere else a person has used a
    /// computer.
    ///
    /// It also pins the half that is easy to lose in a rewrite: **the identity
    /// survives the promotion.** A peek that was renumbered on its way into the
    /// list would orphan every directory read still in flight for it, and the
    /// tree the user just decided to keep would stop filling in.
    ///
    /// Red gate / mutation: have `promote` mint a fresh epoch, or have it splice
    /// the window in at `pinned[0]` instead of pushing — either fails here.
    #[test]
    fn a_promoted_peek_joins_the_pinned_list_on_top_of_it() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let standing = open_pinned_at(
            &mut host,
            Some(TAB),
            "C:/x",
            frame(100.0, 100.0, 264.0, 300.0),
            now,
        );
        let peek = open_peek_at(&mut host, OTHER, "C:/y", now);
        assert_eq!(
            host.promote(),
            Some(peek),
            "and it is still the same window"
        );
        assert_eq!(
            stack(&host),
            vec![standing, peek],
            "two windows stand, the newcomer in front"
        );
        assert!(host.is_pinned(standing), "the older one is untouched");
        assert!(host.is_pinned(peek), "and the newer one has been kept");
        assert_eq!(host.peek_id(), None, "the peek slot is free again");
    }

    /// PIN ③ — **a second ask for a root that already has a window is answered
    /// like any other: a peek is born, and the standing window is not touched.**
    ///
    /// # Two rulings, and this test has now carried both
    ///
    /// **2026-08-12 (morning), 同根去重 — instituted.** When 浮窗多开 landed, a
    /// trigger whose root already had a pinned window *raised* that window rather
    /// than opening a second, by analogy with a browser focusing the tab that
    /// already holds the page. This test asserted that raise.
    ///
    /// **2026-08-12 (same day), 同根去重 — repealed**, and it is the repeal that
    /// is asserted below. Four reasons, and the last is the one that decides it:
    ///
    /// * **Precedent is the other way.** Explorer and Finder both open a second
    ///   window on the same folder without complaint; it is the file manager's
    ///   normal way of comparing two places in one tree.
    /// * **Two windows on one root are not duplicates.** Each is its own viewport
    ///   with its own unfolding, its own selection and its own scroll (G81 makes
    ///   the `{root, open, sel}` a throwaway *per window*), so "the same root"
    ///   says nothing about what the two are showing — which is exactly what the
    ///   companion test `two_windows_on_one_root_unfold_independently` pins.
    /// * **It made hover unpredictable** — the same disease that retired the
    ///   mock-up's 「re-click 关闭」 contract on the morning ruling. A hover that
    ///   produces a peek over most triggers and silently produces nothing over
    ///   the ones whose root happens to be open is a gesture the user cannot
    ///   model, and the failure mode reads exactly like the bug 浮窗多开 was
    ///   built to fix.
    /// * **The costs are lopsided.** A window opened by mistake costs one click
    ///   on `×`. A window *refused* costs the user the thing they asked for, with
    ///   no feedback saying why. Between a cheap wrong answer and an expensive
    ///   one, the design takes the cheap one.
    ///
    /// Red gate / mutation: put the dedup back — have `observe` refuse to arm
    /// when any live window shares the trigger, or have the caller raise instead
    /// of opening — and the peek below never arrives.
    #[test]
    fn a_second_ask_for_a_root_already_open_is_answered_like_any_other() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let box_ = frame(100.0, 100.0, 264.0, 300.0);
        let standing = open_pinned_at(&mut host, Some(TAB), "C:/x", box_, now);

        // The very same trigger, hovered again. It arms, it matures, and it is
        // answered — the standing window is not a reason to say nothing.
        host.observe(Some(TAB), now);
        assert_eq!(
            host.take_due(now + FLY_OPEN),
            Some(TAB),
            "a trigger whose root is already open still answers a hover"
        );
        let peek = open_peek_at(&mut host, TAB, "C:/x", now);
        assert_eq!(
            stack(&host),
            vec![standing, peek],
            "the peek joined; nothing was raised in its place"
        );
        assert_eq!(
            host.live(standing).map(|win| win.frame),
            Some(box_),
            "and the window already showing that root did not move"
        );
    }

    /// PIN — **two windows on one root are two viewports, not two copies.**
    ///
    /// The load-bearing half of repealing 同根去重 (2026-08-12): if the second
    /// window were a copy, refusing to open it would cost nothing and the dedup
    /// would have been right. It is not a copy — G81 gives every float its own
    /// throwaway `{root, open, sel}` — so unfolding a directory in one leaves the
    /// other exactly as it was, which is the whole reason a person opens the same
    /// folder twice.
    ///
    /// The first window is unfolded **before** the second is asked for, which is
    /// the ordering that makes this a test of its own rather than a second
    /// spelling of `a_directory_read_comes_home_to_the_window_that_asked`: the
    /// claim is that opening another window on a root *leaves the one already
    /// there alone*, and a state that is only written after both exist could
    /// never catch a birth that clobbers a sister.
    ///
    /// Red gate / mutation: have `promote` copy the newcomer's `files` over every
    /// pinned window sharing its root (the shape a "keep one view per root"
    /// implementation naturally takes) — `/crates` is wiped out of the first
    /// window and the first assertion reads `(false, true)`.
    #[test]
    fn two_windows_on_one_root_unfold_independently() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let first = open_pinned_at(
            &mut host,
            Some(TAB),
            "C:/x",
            frame(100.0, 100.0, 264.0, 300.0),
            now,
        );
        host.live_mut(first)
            .expect("open")
            .files_mut()
            .expect("a tree tenant")
            .files
            .open
            .insert("/crates".to_owned());

        // The same root asked for again, and kept.
        open_peek_at(&mut host, TAB, "C:/x", now);
        let second = host.promote().expect("the second ask was kept");
        assert_ne!(first, second, "two windows, two identities");
        assert_eq!(stack(&host), vec![first, second], "and both stand");

        host.live_mut(second)
            .expect("open")
            .files_mut()
            .expect("a tree tenant")
            .files
            .open
            .insert("/docs".to_owned());

        let unfolded = |host: &FloatHost, id: FloatId| {
            let files = &host
                .live(id)
                .expect("open")
                .files()
                .expect("a tree tenant")
                .files;
            (files.open.contains("/crates"), files.open.contains("/docs"))
        };
        assert_eq!(
            unfolded(&host, first),
            (true, false),
            "the first window shows what was unfolded in it, and nothing else"
        );
        assert_eq!(
            unfolded(&host, second),
            (false, true),
            "and the second likewise — one root, two answers to \"where am I looking\""
        );
    }

    /// PIN ⑤ — **a press on a buried window brings it to the front, and says so.**
    ///
    /// The return value is the point as much as the reorder is: a raise is a
    /// frame debt, and a click that reordered the world without redrawing it is a
    /// click that visibly did nothing. It reports `false` when the window is
    /// already on top precisely so the caller can *not* repaint then.
    ///
    /// Red gate / mutation: make `raise` return `true` unconditionally (a repaint
    /// every press) or drop the `remove`/`push` (no reorder) — the first fails
    /// the last assertion, the second the middle one.
    #[test]
    fn pressing_a_buried_window_brings_it_to_the_front() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let bottom = open_pinned_at(
            &mut host,
            Some(TAB),
            "C:/x",
            frame(100.0, 100.0, 264.0, 300.0),
            now,
        );
        let middle = open_pinned_at(
            &mut host,
            Some(OTHER),
            "C:/y",
            frame(150.0, 150.0, 264.0, 300.0),
            now,
        );
        let top = open_pinned_at(
            &mut host,
            Some(THIRD),
            "C:/z",
            frame(200.0, 200.0, 264.0, 300.0),
            now,
        );
        assert_eq!(stack(&host), vec![bottom, middle, top]);
        assert!(host.raise(bottom), "it was buried, so the order changed");
        assert_eq!(stack(&host), vec![middle, top, bottom]);
        assert!(
            !host.raise(bottom),
            "and raising the frontmost window owes no frame"
        );
        // Top to bottom is the order a hit test asks in, and it is the reverse.
        assert_eq!(
            host.hit_order().map(|win| win.epoch).collect::<Vec<_>>(),
            vec![bottom, top, middle],
            "the pointer meets the frontmost window first"
        );
    }

    /// PIN ⑥ — **a worker's answer reaches the window that asked for it, and no
    /// other.**
    ///
    /// The epoch was already the tag on `files::FilesHost::Float`; what changes
    /// with more than one window is that it now has to *find* one among several
    /// rather than confirm the only one. A lookup that answered "the float" would
    /// deliver every directory to whichever window happened to be in hand, and
    /// two trees would fill in with each other's contents.
    ///
    /// Red gate / mutation: make `live_mut` ignore its argument and return the
    /// first live window — the second assertion delivers into the wrong tree.
    #[test]
    fn a_directory_read_comes_home_to_the_window_that_asked() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let first = open_pinned_at(
            &mut host,
            Some(TAB),
            "C:/x",
            frame(100.0, 100.0, 264.0, 300.0),
            now,
        );
        let second = open_pinned_at(
            &mut host,
            Some(OTHER),
            "C:/y",
            frame(500.0, 200.0, 264.0, 300.0),
            now,
        );
        let peek = open_peek_at(&mut host, THIRD, "C:/z", now);
        assert_ne!(first, second);
        assert_ne!(second, peek);
        for id in [first, second, peek] {
            assert_eq!(
                host.live_mut(id).map(|win| win.epoch),
                Some(id),
                "every window answers to its own name and not to another's"
            );
        }
        assert_eq!(
            host.live_mut(first)
                .and_then(|win| win.files_mut().map(|files| files.files.root.clone())),
            Some("C:/x".to_owned()),
        );
        assert_eq!(
            host.live_mut(second)
                .and_then(|win| win.files_mut().map(|files| files.files.root.clone())),
            Some("C:/y".to_owned()),
        );
        // A window that has gone takes its traffic with it — the cancellation,
        // arriving as a dropped result.
        host.wipe(second);
        assert!(host.live_mut(second).is_none());
        assert_eq!(stack(&host), vec![first, peek], "and nobody else moved");
    }

    /// PIN — **every window is closed, dragged and dissolved on its own.**
    ///
    /// Rule ⑤ of the ruling, in the two places a list can quietly become a
    /// singleton again: `×` on one window, and the geometry change that dissolves
    /// a transient peek. Neither is allowed to reach the others.
    ///
    /// Red gate / mutation: have `dismiss` ignore its `id` and close the topmost,
    /// or have `wipe_peek` clear the pinned list too.
    #[test]
    fn closing_one_window_leaves_the_others_exactly_where_they_were() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let boxes = [
            frame(100.0, 100.0, 264.0, 300.0),
            frame(500.0, 200.0, 264.0, 300.0),
        ];
        let first = open_pinned_at(&mut host, Some(TAB), "C:/x", boxes[0], now);
        let second = open_pinned_at(&mut host, Some(OTHER), "C:/y", boxes[1], now);
        let peek = open_peek_at(&mut host, THIRD, "C:/z", now);

        assert!(host.dismiss(first, now), "the buried window's own ×");
        assert!(host.live(first).is_none(), "it stopped answering");
        assert_eq!(
            host.live(second).map(|win| win.frame),
            Some(boxes[1]),
            "and the window above it did not budge"
        );
        assert_eq!(host.peek_id(), Some(peek), "nor did the peek");
        assert!(
            host.drawn().any(|win| win.epoch == first),
            "the closing window is still on screen for its exit"
        );

        // §3.2: a geometry change dissolves the TRANSIENT regime and re-clamps
        // the PINNED one. The peek goes; nobody else does.
        assert!(host.wipe_peek().is_some());
        assert_eq!(host.peek_id(), None);
        assert_eq!(
            host.live(second).map(|win| win.frame),
            Some(boxes[1]),
            "dissolving the peek is not a closer for anything else"
        );
    }

    /// PIN — **Esc is aimed at the frontmost window**, which is the peek while
    /// one is up and the top of the list otherwise.
    #[test]
    fn the_top_of_the_stack_is_the_peek_and_then_the_newest_pinned_window() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        assert!(host.top().is_none(), "nothing open, nothing aimed at");
        let first = open_pinned_at(
            &mut host,
            Some(TAB),
            "C:/x",
            frame(100.0, 100.0, 264.0, 300.0),
            now,
        );
        assert_eq!(host.top().map(|win| win.epoch), Some(first));
        let second = open_pinned_at(
            &mut host,
            Some(OTHER),
            "C:/y",
            frame(500.0, 200.0, 264.0, 300.0),
            now,
        );
        assert_eq!(host.top().map(|win| win.epoch), Some(second));
        let peek = open_peek_at(&mut host, THIRD, "C:/z", now);
        assert_eq!(
            host.top().map(|win| win.epoch),
            Some(peek),
            "a peek is the newest and the most temporary thing on screen"
        );
    }

    /// The tooltip's rule: resting on the trigger that is already showing must
    /// not re-arm anything, or a trembling hand closes and reopens it forever.
    #[test]
    fn resting_on_the_trigger_already_showing_arms_nothing() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        open_peek(&mut host, TAB, now);
        host.observe(Some(TAB), now);
        assert_eq!(host.take_due(now + FLY_OPEN * 2), None);
    }

    /// G87: Esc with an intent in flight must take the intent too, or the peek
    /// closes and then reopens by itself under a pointer that never moved.
    #[test]
    fn dismissing_takes_a_pending_intent_with_it() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);
        host.observe(Some(OTHER), now);
        assert!(host.dismiss(peek, now));
        assert_eq!(
            host.take_due(now + FLY_OPEN * 2),
            None,
            "the armed intent died with the peek"
        );
    }

    /// G84: the grace is a clock, and a leftward departure gets the long one.
    #[test]
    fn a_leftward_departure_is_given_the_longer_grace() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        open_peek(&mut host, TAB, now);
        host.release(true, now);
        assert!(
            !host.grace_expired(now + FLY_CLOSE),
            "the short grace is not this one's"
        );
        assert!(host.grace_expired(now + FLY_CLOSE_LEFT));
    }

    /// G85, the bug spelled as a test: a spent clock must not still *look*
    /// pending, because [`FloatHost::release`] refuses to arm a second one while
    /// one is outstanding — so a clock that fired and stayed set would mean the
    /// peek could never be put on the clock again, and it would stay up until Esc.
    ///
    /// The two `release`/`grace_expired` pairs run with **no reopening between
    /// them**, and that is the whole design of the test. An earlier draft opened a
    /// second peek in the middle and passed even with the reset deleted —
    /// [`FloatHost::open`] clears the clock itself, so the reopening was quietly
    /// doing the work the assertion claimed to be checking. A test that cannot
    /// fail is not evidence, and the mutation that proved it is the same one this
    /// version now catches: delete `self.closing_at = None` below and the second
    /// `release` here arms nothing.
    #[test]
    fn a_spent_grace_leaves_the_clock_free_to_be_armed_again() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        open_peek(&mut host, TAB, now);
        host.release(false, now);
        assert!(host.grace_expired(now + FLY_CLOSE), "the first one fires");
        // The pointer came back and left again, without the peek ever having been
        // taken down — the second departure has to get a clock of its own, and
        // **the assertion is about when it fires, not that it does**. A stale
        // clock is a clock that has already expired, so "did it fire" cannot tell
        // a fresh one from a leftover; only "did it wait its full grace" can.
        host.release(false, now + FLY_CLOSE);
        assert!(
            !host.grace_expired(now + FLY_CLOSE),
            "the second grace starts now, so it is not due at the instant it began"
        );
        assert!(
            host.grace_expired(now + FLY_CLOSE * 2),
            "and it comes due a full grace later"
        );
    }

    /// A pinned window is not on a clock at all: §7.1.2's four closers are the
    /// only ones, and a pointer leaving is not among them.
    #[test]
    fn a_pinned_window_is_never_put_on_the_grace_clock() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);
        assert!(host.promote().is_some());
        host.release(false, now);
        assert!(!host.grace_expired(now + FLY_CLOSE_LEFT * 4));
        assert!(host.is_pinned(peek), "and it is still there");
    }

    /// PIN (user ruling 2026-08-12, rule ②) — **promotion stops both clocks.**
    ///
    /// Dragging a peek's header keeps it, and the moment it is kept it stops
    /// being a peek: nothing that could take a peek away may survive. There are
    /// two such things and the grace above is only one of them. The other is a
    /// *pending intent* — the pointer rested on a trigger 180ms ago and the timer
    /// is still running — and it is newly reachable here, because the drag that
    /// promotes begins with the pointer somewhere it may well have armed one.
    ///
    /// Left alive it would mature a moment later and `open` a second float over
    /// the window the user had just decided to keep, mid-carry.
    ///
    /// Red gate / mutation: delete `self.disarm()` from `promote` and the last
    /// assertion here hands back a trigger that is about to reopen the window.
    #[test]
    fn promoting_stops_the_grace_and_the_intent_alike() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);

        // Both clocks running: the pointer has left (a grace is armed) and it has
        // since come to rest on another trigger (an intent is arming).
        host.release(false, now);
        host.observe(Some(OTHER), now);

        assert!(host.promote().is_some(), "the header drag keeps it");
        assert!(
            !host.grace_expired(now + FLY_CLOSE_LEFT * 4),
            "the grace is out: a window being carried is not one that times out"
        );
        assert_eq!(
            host.take_due(now + FLY_OPEN * 4),
            None,
            "and the intent is out: it would have opened a second float over this one"
        );
        assert!(
            host.is_pinned(peek),
            "leaving exactly the window that was kept"
        );
    }

    /// G91: promotion happens in place, so the tree you unfolded stays unfolded
    /// and the window stays where it is.
    #[test]
    fn promoting_a_peek_keeps_its_place_and_its_state() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let mut opened = state("C:/x");
        opened.open.insert("/crates".to_owned());
        let peek = host.open(
            FloatMode::Peek,
            Some(TAB),
            FloatTenant::Files(Box::new(FloatFiles {
                files: opened,
                width: bt_layout::LogicalPx::px(240),
                ..FloatFiles::default()
            })),
            frame(180.0, 90.0, 264.0, 300.0),
            None,
            now,
        );
        assert_eq!(host.promote(), Some(peek));
        let win = host.live(peek).expect("still open");
        assert_eq!(win.mode, FloatMode::Pinned);
        assert_eq!(win.frame, frame(180.0, 90.0, 264.0, 300.0), "same box");
        assert!(
            win.files()
                .expect("a tree tenant")
                .files
                .open
                .contains("/crates"),
            "same unfolding"
        );
        assert!(win.focused, "and a click hands it the keyboard");
    }

    /// G90: a peek never holds the keyboard. Hovering must not take the terminal's
    /// keys away.
    #[test]
    fn a_peek_never_holds_the_keyboard() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);
        assert!(!host.live(peek).expect("open").focused);
    }

    /// G78: a dismissed float stays on screen for its exit but stops answering
    /// anything — `pointer-events: none` on `.closing`, as a state rather than a
    /// second flag.
    #[test]
    fn a_dismissed_float_is_still_drawn_but_no_longer_live() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);
        assert!(host.dismiss(peek, now));
        assert!(host.live(peek).is_none(), "it answers nothing");
        assert_eq!(stack(&host), vec![peek], "but it is still on screen");
        assert!(
            !host.sweep(now, Motion::Full, SCALE),
            "and it is not retired mid-animation"
        );
        assert!(host.sweep(now + FLOAT_ANIMATION, Motion::Full, SCALE));
        assert!(stack(&host).is_empty(), "then it is gone");
    }

    /// Reduced motion turns both keyframes off outright, so there is no exit to
    /// wait through and nothing owes a frame.
    #[test]
    fn reduced_motion_retires_a_dismissed_float_at_once() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);
        host.dismiss(peek, now);
        assert!(host.sweep(now, Motion::Reduced, SCALE));
        assert_eq!(
            host.deadline(now, Motion::Reduced, SCALE, FLOAT_ANIMATION),
            None
        );
    }

    /// PIN — and the *entrance* is off under reduced motion too, which the
    /// retirement test above cannot see.
    ///
    /// The red gate: `sweep` and `deadline` only say that nothing further is
    /// owed. A `fade` that kept its five-pixel rise under reduced motion would
    /// satisfy both of them and still leave every peek permanently displaced
    /// upward, with no animation left to bring it down — a window drawn in the
    /// wrong place forever, because the thing that used to move it was turned
    /// off rather than completed.
    #[test]
    fn reduced_motion_gives_a_peek_no_entrance_to_play() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);
        let born = host
            .drawn()
            .next()
            .expect("open")
            .fade(now, Motion::Reduced, SCALE);
        assert_eq!(born.opacity, 1.0, "it is simply there on the first frame");
        assert_eq!(born.rise, 0.0, "and it is there in its final place");
        assert!(!born.moving, "with nothing owed");
        host.dismiss(peek, now);
        let leaving = host
            .drawn()
            .next()
            .expect("closing")
            .fade(now, Motion::Reduced, SCALE);
        assert_eq!(leaving.opacity, 0.0, "and gone the same way");
        assert!(!leaving.moving);
    }

    /// The entrance rises into place, and the exit falls back the way it came.
    #[test]
    fn the_entrance_rises_and_the_exit_reverses_it() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);
        let born = host
            .drawn()
            .next()
            .expect("open")
            .fade(now, Motion::Full, SCALE);
        assert_eq!(born.opacity, 0.0, "it starts invisible");
        assert_eq!(
            born.rise, FLOAT_WINDOW_RISE_LOGICAL_PX,
            "and five pixels high"
        );
        let landed =
            host.drawn()
                .next()
                .expect("open")
                .fade(now + FLOAT_ANIMATION, Motion::Full, SCALE);
        assert_eq!(landed.opacity, 1.0);
        assert_eq!(landed.rise, 0.0);
        assert!(!landed.moving);
        host.dismiss(peek, now + FLOAT_ANIMATION);
        let leaving =
            host.drawn()
                .next()
                .expect("closing")
                .fade(now + FLOAT_ANIMATION, Motion::Full, SCALE);
        assert_eq!(
            leaving.opacity, 1.0,
            "the exit starts where the entrance ended"
        );
    }

    /// The epoch is what lets a worker's answer be matched to the view that asked
    /// — a peek redirected to another trigger must not be handed the old root's
    /// directories.
    #[test]
    fn every_opening_mints_a_new_epoch() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let first = open_peek(&mut host, TAB, now);
        let second = open_peek(&mut host, OTHER, now);
        assert_ne!(second, first);
        assert!(
            host.live(first).is_none(),
            "and the peek slot holds one question at a time"
        );
    }

    /// A float opens still sizing itself, and **a hand is what ends that**.
    ///
    /// The bug this pins was found on the real machine, not here: the window's
    /// height was measured once, at birth, when its root had not been read yet —
    /// so a peek opened as a bare strip and stayed one however many rows the
    /// worker delivered a moment later. The mock-up could never show it, because
    /// its tree is a literal in the same file and is therefore always ready in
    /// the frame the window opens.
    ///
    /// The other half is the one this asserts second: `height: auto` loses to an
    /// inline height, so once a drag or a resize has happened the size is the
    /// user's answer and later rows do not get to move a window somebody put
    /// somewhere.
    #[test]
    fn a_float_sizes_itself_until_a_hand_takes_hold_of_it() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);
        assert!(
            host.live(peek).expect("open").self_sizing,
            "a fresh float is still following its content"
        );
        host.live_mut(peek).expect("open").self_sizing = false;
        assert!(
            !host.live(peek).expect("open").self_sizing,
            "and a hand on it ends that for good"
        );
    }

    /// Promotion ends peek-hood, and with it everything the peek slot answers
    /// for — the dismissal grace above all: a window the hand has kept must not
    /// be carried off by a clock that was started while it was still a question.
    ///
    /// The rail's half of G102 is **not** here, and deliberately so since
    /// 2026-08-15: which peeks hold the rail open is a question about the peek's
    /// trigger, and it is asked in `main.rs`'s `rail_zone_wants_open`. See
    /// [`FloatHost::peek_is_open`].
    #[test]
    fn promotion_empties_the_peek_slot() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let peek = open_peek(&mut host, TAB, now);
        assert!(host.peek_is_open(), "a peek is in the slot");
        assert!(host.promote().is_some());
        assert!(!host.peek_is_open(), "a pinned window has left it");
        assert!(host.is_pinned(peek));
    }

    // ── the second tenant (P43-P67) and the cascade ─────────────────────────

    /// PIN — P45/P49/P65: **one chassis, two sets of numbers.** Every value the
    /// mock-up gives `#pv-float` differs from `#files-flyout`'s, and P49's whole
    /// point is that "shared chassis" must not be read as "same dimensions".
    ///
    /// Mutation: return `FloatSizing::files()` from `FloatSizing::preview` and
    /// every assertion here goes red at once.
    #[test]
    fn the_preview_float_is_a_wider_taller_window_than_the_files_flyout() {
        let files = FloatSizing::files();
        let preview = FloatSizing::preview();
        assert_eq!((files.width, preview.width), (264.0, 430.0));
        assert_eq!((files.max_height, preview.max_height), (460.0, 520.0));
        assert_eq!(
            (files.max_height_fraction, preview.max_height_fraction),
            (0.62, 0.64)
        );
        assert_eq!((files.min_width, preview.min_width), (200.0, 260.0));
        assert_eq!((files.min_height, preview.min_height), (150.0, 200.0));

        // And the numbers are *used*, not merely declared: the opening size and
        // the grip's floor both read them.
        let viewport = [0.0, 34.0, 1600.0, 1000.0];
        assert_eq!(
            float_opening_size(100_000.0, viewport, SCALE, preview)[0],
            430.0,
            "a preview float opens 430 wide"
        );
        let squeezed = float_resized_to(
            frame(100.0, 100.0, 430.0, 400.0),
            [110.0, 110.0],
            VIEWPORT,
            SCALE,
            preview,
        );
        assert_eq!(
            [squeezed[2] - squeezed[0], squeezed[3] - squeezed[1]],
            [260.0, 200.0],
            "and the grip cannot take it below 260×200"
        );
    }

    /// PIN — P57: the preview head reserves the dot's slot and the two buffer
    /// verbs, and it drops them from the left of the trailing run when the head
    /// runs out of width — with the `×` the last thing standing.
    ///
    /// Mutation: reserve the dot *after* the title instead of before it, and the
    /// third assertion goes red — the name's box runs under the dot.
    #[test]
    fn the_preview_heads_own_controls_take_their_room_before_the_name_does() {
        let box_ = frame(100.0, 100.0, 430.0, 400.0);
        let bare = float_geometry(
            box_,
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools::default(),
        );
        assert_eq!((bare.dirty, bare.save, bare.flip), (None, None, None));

        let dressed = float_geometry(
            box_,
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools {
                dirty: true,
                save: true,
                flip: true,
            },
        );
        let dirty = dressed.dirty.expect("the dot's slot is reserved");
        let save = dressed.save.expect("a text buffer wears a save button");
        let flip = dressed.flip.expect("markdown wears a flip");
        let dock = dressed.dock.expect("and DOCK is still there");
        assert!(
            save[2] <= flip[0] && flip[2] <= dock[0] && dock[2] <= dressed.close[0],
            "DOM order left to right: save, flip, DOCK, ×"
        );
        assert!(
            dirty[2] <= save[0],
            "the dot stands between the name and the run"
        );
        assert!(
            dressed.head_title[2] <= dirty[0],
            "and the name's box stops before the dot rather than under it"
        );
        assert!(
            dressed.head_title[2] < bare.head_title[2],
            "a dressed head has less room for the name than a bare one"
        );
    }

    /// PIN — P57/P54: a preview float's head is hit-tested for its own two
    /// controls, and they are asked before the head that carries them.
    ///
    /// Mutation: move the `Save`/`Flip` arms below the `Head` test in
    /// [`float_hit`] and pressing a button drags the window instead.
    #[test]
    fn the_preview_heads_buttons_answer_before_the_drag_handle_does() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 430.0, 400.0),
            FloatMode::Pinned,
            SCALE,
            30.0,
            FloatHeadTools {
                dirty: true,
                save: true,
                flip: true,
            },
        );
        let middle = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        let (x, y) = middle(geometry.save.expect("a save button"));
        assert_eq!(
            float_hit(&geometry, x, y, |_, _| None),
            Some(FloatPart::Save)
        );
        let (x, y) = middle(geometry.flip.expect("a flip button"));
        assert_eq!(
            float_hit(&geometry, x, y, |_, _| None),
            Some(FloatPart::Flip)
        );
        let (x, y) = middle(geometry.dirty.expect("a reserved dot"));
        assert_eq!(
            float_hit(&geometry, x, y, |_, _| None),
            Some(FloatPart::Head),
            "the dot is a state and not a control — the head under it still drags"
        );
    }

    /// PIN — the 2026-08-12 cascade ruling: a window born on top of one that is
    /// already open steps down and right instead of hiding it.
    ///
    /// Mutation ①: return `frame` unchanged and the second assertion goes red —
    /// two windows at one origin. Mutation ②: step both axes unconditionally and
    /// the third block goes red — a window with no room below is walked off the
    /// bottom of the screen.
    ///
    /// **The edge cases were re-judged on 2026-08-13.** This used to assert that
    /// a window with no room *below* rolled all the way back to its placement —
    /// onto the very window it was told about. That is the roll-back firing on
    /// the first rung, and it is what shipped as two pop-outs at one box; see
    /// [`cascade_origin`]'s own note. A blocked axis now simply stops stepping
    /// while the other one carries on, and the roll-back is kept for the corner
    /// where both are blocked, which is the case it was always arguing about.
    #[test]
    fn a_float_born_on_top_of_another_steps_down_and_right() {
        let viewport = [0.0, 34.0, 1280.0, 800.0];
        let born = frame(100.0, 100.0, 430.0, 300.0);
        assert_eq!(
            cascade_origin(born, &[], viewport, SCALE),
            born,
            "an empty screen takes the placement it was given"
        );
        let one = cascade_origin(born, &[[100.0, 100.0]], viewport, SCALE);
        assert_eq!(
            [one[0], one[1]],
            [124.0, 124.0],
            "one step of the title bar's own height"
        );
        let two = cascade_origin(born, &[[100.0, 100.0], [124.0, 124.0]], viewport, SCALE);
        assert_eq!([two[0], two[1]], [148.0, 148.0], "and the ladder continues");
        assert_eq!(
            [one[2] - one[0], one[3] - one[1]],
            [430.0, 300.0],
            "a step moves the window, it does not resize it"
        );
        // A window with no room *below* still has room to its right, so it goes
        // there: a blocked axis stops that axis and nothing else.
        let low = frame(100.0, 470.0, 430.0, 300.0);
        let sideways = cascade_origin(low, &[[100.0, 470.0]], viewport, SCALE);
        assert_eq!(
            [sideways[0], sideways[1]],
            [124.0, 470.0],
            "no room below: the ladder turns sideways rather than giving up"
        );
        // And the mirror of it, which is the case a preview float is always in.
        let right = frame(834.0, 100.0, 430.0, 300.0);
        let downward = cascade_origin(right, &[[834.0, 100.0]], viewport, SCALE);
        assert_eq!(
            [downward[0], downward[1]],
            [834.0, 124.0],
            "flush against the right margin: the ladder goes straight down"
        );
        // Only the far corner has nowhere left, and there the ladder wraps to
        // where the placement put it rather than piling against the edge.
        let corner = frame(834.0, 470.0, 430.0, 300.0);
        let rolled = cascade_origin(corner, &[[834.0, 470.0]], viewport, SCALE);
        assert_eq!(
            [rolled[0], rolled[1]],
            [834.0, 470.0],
            "both axes blocked: back to where the placement put it"
        );
    }

    /// PIN — **the cascade as it is actually called**, for a window born against
    /// the right margin.
    ///
    /// The function above was tested in isolation and was right about every case
    /// it was asked; what shipped broken was the *pipeline* — `float_placement`,
    /// then `cascade_origin`, then `clamp_pinned` — because a preview pops out of
    /// a pane head at the far right of the tree, so `float_placement` clamps it
    /// flush against `viewport[2] - margin` and the diagonal step has nowhere to
    /// go on its very first try. Real-machine capture, 2026-08-13: two pop-outs
    /// landed at one box to the pixel and the first window was unreachable.
    ///
    /// So this walks the whole pipeline rather than the middle of it, and it uses
    /// a right-edge trigger because that is the case a preview float always is.
    ///
    /// Red gate: roll back to the un-stepped origin whenever the diagonal leaves
    /// the viewport — the shape this had — and the second window lands on the
    /// first, which is the assertion below.
    #[test]
    fn two_windows_popped_out_at_the_right_margin_do_not_land_on_each_other() {
        const HIDPI: f32 = 2.0;
        let viewport = [0.0, 32.0, 2740.0, 1660.0];
        // The `.pv-popout` button of a preview pane at the far right of the tree.
        let trigger = frame(2600.0, 60.0, 40.0, 30.0);
        let size = float_opening_size(100_000.0, viewport, HIDPI, FloatSizing::preview());
        let place = |taken: &[[f32; 2]]| {
            let placed = float_placement(trigger, size, viewport, HIDPI);
            let stepped = cascade_origin(placed, taken, viewport, HIDPI);
            clamp_pinned(stepped, viewport, HIDPI)
        };
        let first = place(&[]);
        assert!(
            (first[2] - (viewport[2] - FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX * HIDPI)).abs()
                <= 4.0,
            "the fixture is only honest if the first window really is flush right: {first:?}"
        );
        let second = place(&[[first[0], first[1]]]);
        assert_ne!(
            [second[0], second[1]],
            [first[0], first[1]],
            "a second pop-out must be visible as a second window"
        );
        // Sideways is what has no room here, so the step is the one axis that
        // does — and the window is moved, not resized.
        assert_eq!(
            [second[2] - second[0], second[3] - second[1]],
            [first[2] - first[0], first[3] - first[1]],
            "a step moves the window, it does not resize it"
        );
        assert!(
            second[1] > first[1],
            "flush against the right margin, the ladder goes down: {second:?}"
        );
        assert!(
            second[3] <= viewport[3],
            "and stays on the screen it stepped inside of: {second:?}"
        );
    }

    /// PIN — **two pop-outs from one address open two windows, one step apart.**
    ///
    /// `Runtime::pop_out_preview`'s whole placement decision, driven twice against
    /// a live host: read the origins already taken, place against the button,
    /// cascade, clamp, open. The anchor is deliberately the *same* both times,
    /// which is the case the machine found — the pop-out button is right-aligned
    /// in a preview head, so a pane that widens when its neighbour leaves offers
    /// the button at the very same x.
    ///
    /// It is written against the host rather than against `cascade_origin` alone
    /// because what shipped broken was never the arithmetic: the step is only
    /// worth anything if the newcomer is a *second* window and if the list it is
    /// stepping over is the one the host actually holds.
    ///
    /// Mutation ①: hand `cascade_origin` an empty `taken` — the second window
    /// lands on the first and the offset assertion goes red. Mutation ②: reuse
    /// the first window instead of opening a second (`wipe` before `open`) — the
    /// stack assertion goes red, which is the reuse-versus-pop-out ruling of
    /// 2026-08-13 stated as a test.
    #[test]
    fn two_pop_outs_from_one_button_open_two_windows_one_step_apart() {
        const HIDPI: f32 = 2.0;
        let now = Instant::now();
        let viewport = [0.0, 32.0, 2740.0, 1660.0];
        // An interior head, so the ladder has room on both axes and the step is
        // the diagonal the ruling describes. The right-margin case — where a
        // preview pane's head usually is — is pinned by its own test above.
        let trigger = frame(1200.0, 120.0, 40.0, 30.0);
        let size = float_opening_size(100_000.0, viewport, HIDPI, FloatSizing::preview());
        let mut host = FloatHost::default();
        // Exactly the six lines `pop_out_preview` runs, in its order.
        let pop_out = |host: &mut FloatHost| {
            let taken: Vec<[f32; 2]> = host
                .live_windows()
                .map(|win| [win.frame[0], win.frame[1]])
                .collect();
            let placed = float_placement(trigger, size, viewport, HIDPI);
            let stepped = cascade_origin(placed, &taken, viewport, HIDPI);
            let frame = clamp_pinned(stepped, viewport, HIDPI);
            host.open(
                FloatMode::Pinned,
                None,
                FloatTenant::Preview(FloatPreview { tab: TAB_ID }),
                frame,
                None,
                now,
            )
        };
        let first = pop_out(&mut host);
        let second = pop_out(&mut host);
        assert_ne!(
            first, second,
            "a pop-out opens a window; it never reuses the one already torn off"
        );
        assert_eq!(
            stack(&host),
            vec![first, second],
            "and both stand — the second did not replace the first"
        );
        let one = host.live(first).expect("the first window").frame;
        let two = host.live(second).expect("the second window").frame;
        assert_eq!(
            [two[0] - one[0], two[1] - one[1]],
            [
                FLOAT_CASCADE_STEP_LOGICAL_PX * HIDPI,
                FLOAT_CASCADE_STEP_LOGICAL_PX * HIDPI
            ],
            "one step down and right, so the newcomer is visible as a newcomer"
        );
        assert_eq!(
            [two[2] - two[0], two[3] - two[1]],
            [one[2] - one[0], one[3] - one[1]],
            "a step moves the window, it does not resize it"
        );
        // And a third, so the ladder is a ladder rather than a single nudge.
        let third = pop_out(&mut host);
        let three = host.live(third).expect("the third window").frame;
        assert_eq!(
            [three[0] - two[0], three[1] - two[1]],
            [
                FLOAT_CASCADE_STEP_LOGICAL_PX * HIDPI,
                FLOAT_CASCADE_STEP_LOGICAL_PX * HIDPI
            ],
            "each newcomer steps clear of every window already standing"
        );
    }

    /// PIN — **an undocked column opens at its size, not as a strip** (user
    /// report, 2026-08-16).
    ///
    /// `Runtime::undock_files_column` used to anchor the new window to the
    /// column's whole rectangle. A docked column runs from the top of the content
    /// area to its foot, so `float_placement` — asked to stand the window above
    /// or below its trigger — had no room on either side and took its last
    /// resort: a window one strip tall. This drives the placement twice against
    /// the same column geometry, once anchored the old way and once to the
    /// `.pane-float` button in the column's own head, which is where the
    /// runtime now anchors it. The first is the strip the report shows; the
    /// second is the window that was asked for.
    ///
    /// Mutation: anchor the runtime to `full_pane_rect` again and the machine
    /// reproduces the first half of this test.
    #[test]
    fn a_column_undocked_from_its_head_button_opens_at_its_size_and_not_as_a_strip() {
        const HIDPI: f32 = 2.0;
        let viewport = [0.0, 88.0, 2740.0, 1660.0];
        // A docked files column: the viewport's full height, at the left edge.
        let column = [0.0, 88.0, 528.0, 1660.0];
        let size = float_opening_size(
            crate::seats::files_tree_content_height(15, HIDPI),
            viewport,
            HIDPI,
            FloatSizing::files(),
        );
        assert!(
            size[1] > FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX * HIDPI * 4.0,
            "fifteen rows are a real body, not a strip: {size:?}"
        );

        let anchored_to_column = float_placement(column, size, viewport, HIDPI);
        assert_eq!(
            anchored_to_column[3] - anchored_to_column[1],
            FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX * HIDPI,
            "anchored to the whole column there is no room on either side of it, and the last resort is a strip — the bug"
        );

        let button = crate::seats::pane_head_geometry(column, bt_layout::SeatKind::Files, HIDPI)
            .float
            .expect("a files head offers its pop-out button");
        let anchored_to_button = float_placement(button, size, viewport, HIDPI);
        let frame = clamp_pinned(anchored_to_button, viewport, HIDPI);
        assert_eq!(
            [frame[2] - frame[0], frame[3] - frame[1]],
            size,
            "anchored to the button in its head, the window opens at the size its rows asked for"
        );
        assert!(
            frame[1] >= button[3],
            "and hangs under the button that summoned it: {frame:?} under {button:?}"
        );
    }

    /// PIN — a preview float is a tenant of this host like any other: it takes a
    /// place in the pinned list, answers to its own id, and does not disturb the
    /// tree that was already floating (P44's "one chassis, many tenants").
    ///
    /// Mutation: have `FloatWin::preview()` answer `Some` for a files tenant and
    /// the last two assertions cross over.
    #[test]
    fn a_preview_float_stands_beside_a_files_float_without_disturbing_it() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let tree = open_pinned_at(
            &mut host,
            Some(TAB),
            "C:/x",
            frame(100.0, 100.0, 264.0, 300.0),
            now,
        );
        let buffer = host.open(
            FloatMode::Pinned,
            None,
            FloatTenant::Preview(FloatPreview { tab: TAB_ID }),
            frame(300.0, 200.0, 430.0, 400.0),
            None,
            now,
        );
        assert_eq!(stack(&host), vec![tree, buffer], "both stand, in order");
        assert_eq!(
            host.top().map(|win| win.epoch),
            Some(buffer),
            "and Esc is aimed at the newer one — P66's ladder read as a z-order"
        );
        assert!(host.live(tree).expect("open").files().is_some());
        assert!(host.live(tree).expect("open").preview().is_none());
        assert!(host.live(buffer).expect("open").preview().is_some());
        assert!(host.live(buffer).expect("open").files().is_none());

        // Closing the buffer's window leaves the tree's exactly as it was.
        host.wipe(buffer);
        assert_eq!(stack(&host), vec![tree]);
        assert_eq!(
            host.live(tree).expect("open").frame,
            frame(100.0, 100.0, 264.0, 300.0)
        );
    }

    /// PIN (P67, ruling 7) — **the preview float is re-clamped by the window's
    /// resize, and it inherits that rather than being given a second copy of it.**
    ///
    /// P67 records a gap in the mock-up: `#files-flyout` has a `resize` listener
    /// that hauls a pinned window back into the viewport, and `#pv-float` has
    /// none — so the same bug was waiting to be re-committed on the second window.
    /// The native chassis closes it by construction: `Runtime::reclamp_float`
    /// walks [`FloatHost::live_windows_mut`] and puts every frame through
    /// [`clamp_pinned`], and nothing in either of them asks what is inside.
    ///
    /// This pins the two halves of that sentence — the walk reaches a preview
    /// tenant, and the clamp answers for it — because "inherits it" is exactly the
    /// kind of claim that is true until somebody adds a tenant filter.
    ///
    /// Mutation: filter `live_windows_mut` to files tenants and the last two
    /// assertions go red — a torn-off buffer stranded off-screen with its only
    /// handle out of reach.
    #[test]
    fn a_shrinking_window_hauls_the_preview_float_back_like_every_other_float() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let tree = open_pinned_at(
            &mut host,
            Some(TAB),
            "C:/x",
            frame(100.0, 100.0, 264.0, 300.0),
            now,
        );
        let buffer = host.open(
            FloatMode::Pinned,
            None,
            FloatTenant::Preview(FloatPreview { tab: TAB_ID }),
            frame(900.0, 500.0, 430.0, 400.0),
            None,
            now,
        );
        // The window shrinks under both of them — `reclamp_float`'s own loop.
        let shrunk = [0.0, 34.0, 700.0, 600.0];
        for win in host.live_windows_mut() {
            win.frame = clamp_pinned(win.frame, shrunk, SCALE);
        }
        let inside = |frame: [f32; 4]| {
            frame[0] >= shrunk[0]
                && frame[1] >= shrunk[1]
                && frame[2] <= shrunk[2]
                && frame[3] <= shrunk[3]
        };
        assert!(inside(host.live(tree).expect("open").frame));
        let buffer_frame = host.live(buffer).expect("open").frame;
        assert!(
            inside(buffer_frame),
            "the torn-off buffer came back too: {buffer_frame:?}"
        );
        assert_eq!(
            [
                buffer_frame[2] - buffer_frame[0],
                buffer_frame[3] - buffer_frame[1]
            ],
            [430.0, 400.0],
            "translated, not shrunk — the ruling is 平移回视口、尺寸不变"
        );
    }
}
