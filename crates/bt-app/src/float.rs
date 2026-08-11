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
    let dock_right = close[0] - gap;
    let dock_left = dock_right - dock_box;
    // A head with no room for the button does without it rather than drawing it
    // on top of the name: `×` is the one control that must never be crowded out,
    // because it is the only one that can undo the squeeze.
    let dock = (dock_left > head_mark[2] + gap)
        .then(|| snap([dock_left, dock_top, dock_right, dock_bottom]));

    let title_right = dock.map_or(close[0], |dock| dock[0]) - gap;
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

/// The size a float opens at, before any hand has touched it.
///
/// Width is the design's 264 flat. Height is the content's own, capped by
/// `min(62vh, 460px)` and floored at a strip — see
/// [`bt_render::FLOAT_WINDOW_MAX_HEIGHT_LOGICAL_PX`] for why the cap applies to
/// a fresh pinned window too.
#[must_use]
pub fn float_opening_size(content_height: f32, viewport: [f32; 4], scale: f32) -> [f32; 2] {
    let px = |logical: f32| logical * scale;
    let margin = px(FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX);
    let viewport_height = (viewport[3] - viewport[1] - margin * 2.0).max(0.0);
    let cap = (viewport_height * FLOAT_WINDOW_MAX_HEIGHT_VIEWPORT_FRACTION)
        .min(px(FLOAT_WINDOW_MAX_HEIGHT_LOGICAL_PX));
    let floor = px(FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX);
    let width = px(FLOAT_WINDOW_WIDTH_LOGICAL_PX)
        .min((viewport[2] - viewport[0] - margin * 2.0).max(floor));
    [
        width.round(),
        content_height.clamp(floor, cap.max(floor)).round(),
    ]
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

/// Where a fresh float is placed relative to the trigger that summoned it (G89).
///
/// Under the trigger, left-aligned to it, six pixels down; flipped **above** it
/// when there is no room below; and horizontally clamped — never flipped —
/// which is `peek_box_layout`'s rule and the shape
/// `M2-tiny-window-priority.md` §3.3 asks every float to copy: 主轴翻转、次轴
/// clamp.
#[must_use]
pub fn float_placement(
    trigger: [f32; 4],
    size: [f32; 2],
    viewport: [f32; 4],
    scale: f32,
) -> [f32; 2] {
    let px = |logical: f32| logical * scale;
    let margin = px(FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX);
    let gap = px(FLOAT_WINDOW_TRIGGER_GAP_LOGICAL_PX);
    let left_limit = viewport[0] + margin;
    let right_limit = viewport[2] - margin - size[0];
    let left = trigger[0].clamp(left_limit, right_limit.max(left_limit));
    let mut top = trigger[3] + gap;
    if top + size[1] > viewport[3] - margin {
        top = trigger[1] - gap - size[1];
    }
    let top_limit = viewport[1] + margin;
    let bottom_limit = viewport[3] - margin - size[1];
    [
        left.round(),
        top.clamp(top_limit, bottom_limit.max(top_limit)).round(),
    ]
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
) -> [f32; 4] {
    let px = |logical: f32| logical * scale;
    let margin = px(FLOAT_WINDOW_DRAG_MARGIN_LOGICAL_PX);
    let min_width = px(FLOAT_WINDOW_MIN_WIDTH_LOGICAL_PX);
    let min_height = px(FLOAT_WINDOW_MIN_HEIGHT_LOGICAL_PX);
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
    /// The `×`.
    Close,
    /// A row of the tree, by visible index.
    Row(usize),
    /// The body, but not on a row.
    Body,
    /// The foot, which reveals the folder in the OS file manager.
    Foot,
    /// The corner grip.
    Grip,
}

/// Resolve a pointer against the chassis, smallest target first.
///
/// `row` is the tenant's own hit test — for the files tree,
/// `FilesTreeGeometry::row_at` — passed in rather than performed here, because
/// the chassis does not know what is inside it and that is the whole point of it
/// being a chassis.
#[must_use]
pub fn float_hit(
    geometry: &FloatGeometry,
    x: f32,
    y: f32,
    row: impl Fn(f32, f32) -> Option<usize>,
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
        return Some(
            row(x - geometry.body[0], y - geometry.body[1]).map_or(FloatPart::Body, FloatPart::Row),
        );
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

// ── the singleton ───────────────────────────────────────────────────────────

/// The one float this window may have open, and the two clocks around it.
///
/// **A singleton by ruling** (§7.1.2「全窗口单例」): at most one float at a time,
/// and operating another trigger *redirects* the existing one rather than
/// opening a second. There is no list here because there is no list in the
/// design — a second floating tree would be two answers to "where am I", and the
/// whole reason this form exists is to give one.
#[derive(Debug, Default)]
pub struct FloatHost {
    /// A trigger the pointer is resting on, and when its peek comes due.
    settling: Option<(FloatTrigger, Instant)>,
    /// The float on screen, live or playing its exit.
    open: Option<FloatWin>,
    /// When a transient peek's grace runs out.
    ///
    /// **Time, not space** (G84): the peek is not held open by a dead zone the
    /// pointer must stay inside, it is dismissed by a clock that the pointer
    /// keeps resetting while it is near. And there is deliberately no "focus is
    /// inside, so keep it" guard — a transient peek is never keyboard-driven, so
    /// the only way focus lands in one is an incidental click on a row, and that
    /// must not wedge it open after the pointer has gone. A peek you want to
    /// keep is one you pin.
    closing_at: Option<Instant>,
    /// Bumped on every open, so an answer from the worker that was asked for a
    /// float which has since been replaced can be recognised and dropped.
    epoch: u64,
}

/// The float itself: what it is showing, where it is, and how it got there.
#[derive(Debug)]
pub struct FloatWin {
    pub mode: FloatMode,
    /// The header it can be re-clicked from, or `None` once torn off a column.
    pub origin: FloatOrigin,
    /// Its own throwaway `{root, open, sel}` (G81) — never a leaf's, never
    /// persisted.
    pub files: crate::seats::FilesLeafState,
    /// Its own directory cache, which is also where its scroll position lives —
    /// so `Dock`/pop-out carry it for free, which is the half of G97 the mock-up
    /// never managed.
    pub cache: crate::files::DirCache,
    /// The column width this view carries between float and dock (F75/G97).
    pub width: bt_layout::LogicalPx,
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
}

impl FloatHost {
    /// The float on screen, if it is still live.
    #[must_use]
    pub fn live(&self) -> Option<&FloatWin> {
        self.open.as_ref().filter(|win| win.is_live())
    }

    /// The same, mutably.
    pub fn live_mut(&mut self) -> Option<&mut FloatWin> {
        self.open.as_mut().filter(|win| win.is_live())
    }

    /// Anything at all that should be drawn, dismissed ones included.
    #[must_use]
    pub fn drawn(&self) -> Option<&FloatWin> {
        self.open.as_ref()
    }

    /// Whether a *pinned* float is up — the state that lets the rail retract and
    /// that a hover must not hijack.
    #[must_use]
    pub fn pinned_is_open(&self) -> bool {
        self.live().is_some_and(|win| win.mode == FloatMode::Pinned)
    }

    /// Whether a *transient* peek is up.
    ///
    /// This is `railBusy`'s clause (G102): a peek hangs off a rail row and floats
    /// out past the rail, so moving onto it must not read as having left the
    /// rail. A pinned window is torn off and free-standing, so it holds nothing
    /// open — hence `!pinned`, spelled here as the mode test.
    #[must_use]
    pub fn peek_is_open(&self) -> bool {
        self.live().is_some_and(|win| win.mode.is_transient())
    }

    /// Note the trigger under the pointer and arm the intent (G86/H112).
    ///
    /// Resting on the trigger whose peek is already up re-arms nothing, for the
    /// tooltip's reason: a trembling hand would otherwise close and reopen it
    /// forever. A pinned window is never hijacked by a hover — it was asked for
    /// explicitly, and a passing pointer does not get to replace it.
    pub fn observe(&mut self, trigger: Option<FloatTrigger>, now: Instant) {
        if self.pinned_is_open() {
            self.settling = None;
            return;
        }
        match trigger {
            Some(trigger) => {
                if self.live().is_some_and(|win| win.origin == Some(trigger)) {
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

    /// Open (or redirect) the float.
    ///
    /// One entry point for all four ways a float comes to exist — hover, click,
    /// re-click on another trigger, and a column popping out — because they
    /// differ only in the two arguments and having four would mean four places to
    /// forget the epoch.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &mut self,
        mode: FloatMode,
        origin: FloatOrigin,
        files: crate::seats::FilesLeafState,
        cache: crate::files::DirCache,
        width: bt_layout::LogicalPx,
        frame: [f32; 4],
        anchor: Option<[f32; 4]>,
        now: Instant,
    ) -> u64 {
        self.settling = None;
        self.closing_at = None;
        self.epoch += 1;
        self.open = Some(FloatWin {
            mode,
            origin,
            files,
            cache,
            width,
            frame,
            focused: mode == FloatMode::Pinned,
            epoch: self.epoch,
            anchor,
            self_sizing: true,
            opened_at: now,
            dismissed_at: None,
        });
        self.epoch
    }

    /// Promote the peek under this trigger into a pinned window, in place (G91).
    ///
    /// In place, and that is the point: the tree you were already looking at
    /// stays exactly as you had unfolded it, at the size and position it is
    /// already at. Reopening it as a new pinned window would reset both, and the
    /// gesture is called "keep this", not "start again".
    pub fn promote(&mut self) -> bool {
        let Some(win) = self.live_mut() else {
            return false;
        };
        if win.mode == FloatMode::Pinned {
            return false;
        }
        win.mode = FloatMode::Pinned;
        win.focused = true;
        self.closing_at = None;
        true
    }

    /// Begin the exit (§7.1.2's four closers, and nothing else).
    ///
    /// Returns whether there was anything to close. The window stays in hand
    /// until [`Self::sweep`] retires it, so the exit can be seen; the state it
    /// was showing is gone from that instant, which is what makes a closing float
    /// non-interactive without a second flag.
    pub fn dismiss(&mut self, now: Instant) -> bool {
        self.disarm();
        self.closing_at = None;
        let Some(win) = self.open.as_mut() else {
            return false;
        };
        if win.dismissed_at.is_some() {
            return false;
        }
        win.dismissed_at = Some(now);
        true
    }

    /// Take the float away outright, without an exit — for the paths where the
    /// thing it was standing on has gone and there is nothing to play the
    /// animation against.
    pub fn wipe(&mut self) -> Option<FloatWin> {
        self.disarm();
        self.closing_at = None;
        self.open.take()
    }

    /// Retire a float whose exit has finished. Returns whether one was retired.
    pub fn sweep(&mut self, now: Instant, motion: Motion, scale: f32) -> bool {
        let finished = self
            .open
            .as_ref()
            .is_some_and(|win| !win.is_live() && !win.fade(now, motion, scale).moving);
        if finished {
            self.open = None;
        }
        finished
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
            .open
            .as_ref()
            .is_some_and(|win| win.fade(now, motion, scale).moving)
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
pub struct FloatBody {
    pub quads: Vec<OverlayQuad>,
    pub labels: Vec<ChromeLabel>,
    pub sprites: Vec<ChromeSprite>,
}

/// Draw the chassis around a body someone else filled in.
///
/// `root_name` is what the head shouts (the root's last segment, upper-cased by
/// the caller — `#files-flyout .fly-head { text-transform: uppercase }`, and the
/// rule is the files head's alone because a *filename* keeps its case); `path` is
/// what the foot says, in full. That division is the mock-up's own note at line
/// 731: the header names the leaf, the foot says where you actually are.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn build(
    geometry: &FloatGeometry,
    mode: FloatMode,
    root_name: &str,
    path: &str,
    dock_label: &str,
    hover: Option<FloatPart>,
    // `revealed`: whether the foot is confirming a reveal rather than showing the
    // path (B24). The caption arrives from the caller, so this module holds no
    // clock and no wording.
    revealed: bool,
    body: FloatBody,
    scale: f32,
    palette: &ChromePalette,
    fade: FloatFade,
) -> OverlayLayer {
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
    crate::settings::push_float_window(
        &mut quads,
        geometry.frame,
        px(FLOAT_WINDOW_RADIUS_LOGICAL_PX),
        px(FLOAT_WINDOW_BORDER_LOGICAL_PX).max(1.0).round(),
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
        quads.push(OverlayQuad {
            rect: geometry.foot,
            color: palette.dialog_hover,
            alpha: 1.0,
        });
    }
    let mut sprites = Vec::new();
    let mut labels = Vec::new();
    sprites.push(ChromeSprite::new(
        ChromeMark::Folder,
        geometry.head_mark,
        palette.accent,
    ));
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
            ChromeMark::DockLeft,
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
    labels.push(ChromeLabel {
        text: path.to_owned(),
        rect: geometry.foot_path,
        font_size_px: px(FLOAT_FOOT_FONT_LOGICAL_PX),
        color: foot_ink,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    });
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

    /// G79: the chassis is the mock-up's, value for value — a 30px head, a 30px
    /// foot, and a body that is whatever is left between them.
    #[test]
    fn the_chassis_gives_its_head_and_foot_thirty_pixels_each_and_the_body_the_rest() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 264.0, 400.0),
            FloatMode::Peek,
            SCALE,
            30.0,
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
            float_geometry(box_, FloatMode::Peek, SCALE, 30.0)
                .grip
                .is_none(),
            "a transient peek has no grip"
        );
        let grip = float_geometry(box_, FloatMode::Pinned, SCALE, 30.0)
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

    /// The `×` is the one control that may not be crowded out: it is the only one
    /// that can undo whatever made the window this narrow.
    #[test]
    fn a_head_with_no_room_drops_the_dock_button_and_keeps_the_close() {
        let geometry = float_geometry(frame(0.0, 0.0, 60.0, 200.0), FloatMode::Pinned, SCALE, 30.0);
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
        assert_eq!(placed, [300.0, 65.0], "left-aligned to the icon, 6px below");
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
        let resized = float_resized_to(float, [110.0, 110.0], VIEWPORT, SCALE);
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

    /// The body hands its rows to the tenant's own hit test, in the body's
    /// coordinates — the chassis does not know what is inside it.
    #[test]
    fn the_body_asks_its_tenant_which_row_the_pointer_is_on() {
        let geometry = float_geometry(
            frame(100.0, 100.0, 264.0, 400.0),
            FloatMode::Peek,
            SCALE,
            30.0,
        );
        let inside = (geometry.body[0] + 10.0, geometry.body[1] + 10.0);
        assert_eq!(
            float_hit(&geometry, inside.0, inside.1, |x, y| {
                assert!(x >= 0.0 && y >= 0.0, "coordinates arrive body-local");
                Some(3)
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

    fn open_peek(host: &mut FloatHost, trigger: FloatTrigger, now: Instant) {
        host.open(
            FloatMode::Peek,
            Some(trigger),
            state("C:/x"),
            crate::files::DirCache::default(),
            bt_layout::LogicalPx::px(240),
            frame(100.0, 100.0, 264.0, 300.0),
            None,
            now,
        );
    }

    const TAB: FloatTrigger = FloatTrigger::Tab(TabId(1));
    const OTHER: FloatTrigger = FloatTrigger::Tab(TabId(2));

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

    /// G86: a pinned window is not hijacked by a passing pointer.
    #[test]
    fn a_hover_never_hijacks_a_pinned_window() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        host.open(
            FloatMode::Pinned,
            Some(TAB),
            state("C:/x"),
            crate::files::DirCache::default(),
            bt_layout::LogicalPx::px(240),
            frame(100.0, 100.0, 264.0, 300.0),
            None,
            now,
        );
        host.observe(Some(OTHER), now);
        assert_eq!(
            host.take_due(now + FLY_OPEN * 2),
            None,
            "the intent was never armed"
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
        open_peek(&mut host, TAB, now);
        host.observe(Some(OTHER), now);
        assert!(host.dismiss(now));
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
        open_peek(&mut host, TAB, now);
        assert!(host.promote());
        host.release(false, now);
        assert!(!host.grace_expired(now + FLY_CLOSE_LEFT * 4));
        assert!(host.pinned_is_open(), "and it is still there");
    }

    /// G91: promotion happens in place, so the tree you unfolded stays unfolded
    /// and the window stays where it is.
    #[test]
    fn promoting_a_peek_keeps_its_place_and_its_state() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        let mut opened = state("C:/x");
        opened.open.insert("/crates".to_owned());
        host.open(
            FloatMode::Peek,
            Some(TAB),
            opened,
            crate::files::DirCache::default(),
            bt_layout::LogicalPx::px(240),
            frame(180.0, 90.0, 264.0, 300.0),
            None,
            now,
        );
        assert!(host.promote());
        let win = host.live().expect("still open");
        assert_eq!(win.mode, FloatMode::Pinned);
        assert_eq!(win.frame, frame(180.0, 90.0, 264.0, 300.0), "same box");
        assert!(win.files.open.contains("/crates"), "same unfolding");
        assert!(win.focused, "and a click hands it the keyboard");
    }

    /// G90: a peek never holds the keyboard. Hovering must not take the terminal's
    /// keys away.
    #[test]
    fn a_peek_never_holds_the_keyboard() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        open_peek(&mut host, TAB, now);
        assert!(!host.live().expect("open").focused);
    }

    /// G78: a dismissed float stays on screen for its exit but stops answering
    /// anything — `pointer-events: none` on `.closing`, as a state rather than a
    /// second flag.
    #[test]
    fn a_dismissed_float_is_still_drawn_but_no_longer_live() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        open_peek(&mut host, TAB, now);
        assert!(host.dismiss(now));
        assert!(host.live().is_none(), "it answers nothing");
        assert!(host.drawn().is_some(), "but it is still on screen");
        assert!(
            !host.sweep(now, Motion::Full, SCALE),
            "and it is not retired mid-animation"
        );
        assert!(host.sweep(now + FLOAT_ANIMATION, Motion::Full, SCALE));
        assert!(host.drawn().is_none(), "then it is gone");
    }

    /// Reduced motion turns both keyframes off outright, so there is no exit to
    /// wait through and nothing owes a frame.
    #[test]
    fn reduced_motion_retires_a_dismissed_float_at_once() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        open_peek(&mut host, TAB, now);
        host.dismiss(now);
        assert!(host.sweep(now, Motion::Reduced, SCALE));
        assert_eq!(
            host.deadline(now, Motion::Reduced, SCALE, FLOAT_ANIMATION),
            None
        );
    }

    /// The entrance rises into place, and the exit falls back the way it came.
    #[test]
    fn the_entrance_rises_and_the_exit_reverses_it() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        open_peek(&mut host, TAB, now);
        let born = host.drawn().expect("open").fade(now, Motion::Full, SCALE);
        assert_eq!(born.opacity, 0.0, "it starts invisible");
        assert_eq!(
            born.rise, FLOAT_WINDOW_RISE_LOGICAL_PX,
            "and five pixels high"
        );
        let landed = host
            .drawn()
            .expect("open")
            .fade(now + FLOAT_ANIMATION, Motion::Full, SCALE);
        assert_eq!(landed.opacity, 1.0);
        assert_eq!(landed.rise, 0.0);
        assert!(!landed.moving);
        host.dismiss(now + FLOAT_ANIMATION);
        let leaving =
            host.drawn()
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
        open_peek(&mut host, TAB, now);
        let first = host.live().expect("open").epoch;
        open_peek(&mut host, OTHER, now);
        assert_ne!(host.live().expect("open").epoch, first);
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
        open_peek(&mut host, TAB, now);
        assert!(
            host.live().expect("open").self_sizing,
            "a fresh float is still following its content"
        );
        host.live_mut().expect("open").self_sizing = false;
        assert!(
            !host.live().expect("open").self_sizing,
            "and a hand on it ends that for good"
        );
    }

    /// G102: a transient peek is the rail's unfinished business; a pinned window,
    /// having been torn off, is not.
    #[test]
    fn only_a_transient_peek_holds_the_rail_open() {
        let now = Instant::now();
        let mut host = FloatHost::default();
        open_peek(&mut host, TAB, now);
        assert!(host.peek_is_open(), "a peek holds it");
        assert!(host.promote());
        assert!(!host.peek_is_open(), "a pinned window lets it go");
        assert!(host.pinned_is_open());
    }
}
