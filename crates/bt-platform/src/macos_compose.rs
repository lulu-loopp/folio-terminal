//! **The window's composition, on macOS** — the AppKit twin of
//! `windows_impl`'s `Compositor` (ticket M4-1, on X-1's measurements).
//!
//! # What a composition tree is here, and why it is not a tree
//!
//! On Windows a window presents through a DirectComposition tree: a target
//! bound to the `HWND`, a root, a visual the swapchain hangs off, and one
//! visual per hosted page underneath it. The page is *under* the swapchain
//! because `IDCompositionVisual::AddVisual` was asked to put it at the
//! beginning of the child list, and Folio's frame writes `a = 0` over the
//! pane's rectangle so that the page shows through.
//!
//! macOS has no tree to build. X-1 measured the whole arrangement: **two
//! sibling subviews in one window**, the page's view added *below* the view
//! wgpu hangs its `CAMetalLayer` on, and a transparent pixel in Folio's frame
//! is then a pixel of the page. Later subviews are in front, so ordering is a
//! property of the subview list rather than of a visual tree, and the
//! `CAMetalLayer` needs no sibling of its own to be composited over.
//!
//! ```text
//! NSWindow
//!  └─ contentView                     ← winit's own view, and flipped
//!      ├─ FolioPageSlotView           ← one per page, added at the bottom
//!      │   └─ the WKWebView (M4-2)
//!      └─ FolioSurfaceView            ← M1-4's, and the CAMetalLayer is on it
//! ```
//!
//! # The alpha rules X-1 settled, and where each of them is kept
//!
//! * **Folio writes premultiplied pixels and declares `PostMultiplied`.** That
//!   is `bt-render`'s half (§13.14 ①) and nothing here may restate it. What
//!   this file owes the same rule is its own colours: a page's floor is a
//!   `CGColor` on the slot's layer, `CGColor` components are **straight**
//!   alpha by definition, and CoreAnimation premultiplies them itself — so
//!   [`straight_srgb`] undoes the premultiplication the caller did rather than
//!   handing the same numbers on.
//! * **The blend is on encoded sRGB bytes, not in linear light.** X-1's
//!   predictions are byte arithmetic and land exactly. So the floor's colour is
//!   made in the **sRGB** space by name — `CGColorCreateSRGB` — and never in a
//!   generic or device space, whose components would be a second encoding of
//!   the same number. This is the place a reader looking for where
//!   anti-aliased edges are judged should start; M2-5 judges the glyph edges
//!   themselves.
//! * **A dropped `wgpu::Surface` leaves its `CAMetalLayer` behind**, so the
//!   platform owns the view and empties it before every rebuild
//!   (`macos_impl::clear_surface_layers`). That door is unchanged by this
//!   ticket and **it cannot reach a page slot**: what it empties is the
//!   *sublayers of the surface view's own layer*, and a slot is a **subview of
//!   the content view**, one level up and on the other side of the hierarchy.
//!   That is the whole of the pin, and it is structural rather than a rule
//!   somebody has to remember —
//!   `a_rebuild_empties_the_surface_view_and_not_the_page_slot` measures it on
//!   the machine.
//!
//! # The thread, and the animations
//!
//! **Every call in this file is AppKit, and AppKit is the main thread's.** The
//! gate is [`macos_impl::window_thread`], the same one M1-3's doors pass
//! through, and a door asked from anywhere else refuses with a sentence naming
//! the thread rather than the platform.
//!
//! **Nothing animates.** A layer-backed view whose frame moves runs
//! CoreAnimation's implicit action for the change — a quarter-second of a pane
//! sliding to where the layout already put it, on every frame that moves one.
//! Every mutation below is therefore made inside a [`CATransaction`] with
//! actions disabled ([`without_animation`]), which is the one place that
//! decision is written down.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindowOrderingMode};
use objc2_core_graphics::CGColor;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::{CALayer, CATransaction};

use crate::macos_impl::{folios_surface_view, window_for, window_thread};
use crate::{NativeWindow, PageVisual, VisualLayer, composition_visual_offset, window_skirt};

// ── the order two siblings stand in ────────────────────────────────────────

/// **Where a new subview goes, given the end of the list [`VisualLayer`] names**
/// — the one place that translation is made (M4-1).
///
/// # The `bool` does not travel, and the enum does
///
/// [`VisualLayer::insert_above_with_null_reference`] is `IDCompositionVisual::AddVisual`'s
/// `insertAbove` argument, and its value is a fact about DirectComposition's
/// child list: `TRUE` with a NULL reference asks for the **beginning** of the
/// list, and the beginning of a DirectComposition child list is the **bottom**
/// of the stack. AppKit's list runs the other way — `NSView.subviews` is
/// back-to-front, so the *end* of that list is the top — and
/// `addSubview:positioned:relativeTo:` does not take a boolean at all. It takes
/// an ordering mode, and with a nil reference view `NSWindowBelow` means behind
/// every sibling and `NSWindowAbove` means in front of every one.
///
/// So the `bool` is deliberately not read here. A reader who has learned
/// "`Bottom` is `TRUE`" and carried that number across would place Folio's page
/// on top of Folio's own frame — the failure the W0′ probe photographed on
/// Windows, arriving on the other platform by way of a constant that happens to
/// be the same shape. What travels is the **enum**, whose two variants say
/// which end of the stack is meant, and this function is the only thing in the
/// macOS arm that turns one into an AppKit argument.
#[must_use]
fn ordering_for(layer: VisualLayer) -> NSWindowOrderingMode {
    match layer {
        VisualLayer::Bottom => NSWindowOrderingMode::Below,
        VisualLayer::Top => NSWindowOrderingMode::Above,
    }
}

// ── the page slot ──────────────────────────────────────────────────────────

define_class!(
    // SAFETY:
    // - `NSView` has no subclassing requirements beyond being used on the main
    //   thread, which it declares itself and which this subclass inherits.
    // - This class does not implement `Drop` and has no ivars.
    #[unsafe(super(NSView))]
    #[name = "FolioPageSlotView"]
    struct PageSlot;

    impl PageSlot {
        /// **The same origin every rectangle in this program is written from.**
        ///
        /// winit's own content view answers `true` here for exactly this reason
        /// (`platform_impl/macos/view.rs`: "winit uses the upper-left corner as
        /// the origin"), and a slot that answered `false` would be a second
        /// convention inside one window — the page's view would be laid out
        /// from the bottom of the pane while everything around it was laid out
        /// from the top, and the two would agree only for a pane whose height
        /// never changed.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// **A slot is a place for a page to stand, never a place a click
        /// lands.**
        ///
        /// The slot lies over Folio's own surface in the pane's rectangle, and
        /// AppKit routes a press to the frontmost view whose `hitTest:` claims
        /// the point. A page that has arrived claims its own presses — that is
        /// `WKWebView`'s business and M4-2's — but a slot with nothing in it
        /// yet would claim them itself and end them there: every click in a
        /// pane whose page has not loaded, and every click in a pane that never
        /// gets one, swallowed by a view with no `mouseDown:`.
        ///
        /// Answering the **subviews' answer, and nil for its own**, is the
        /// whole of the rule: a point inside the page goes to the page, and a
        /// point the page does not want falls through to winit's view, which is
        /// the view this program has always answered presses from
        /// (`macos_impl`'s `FolioSurfaceView` does the same thing one level up
        /// and for the same reason).
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            // SAFETY: `hitTest:` on the superclass, with the point it was
            // given, on the thread AppKit called this method on.
            let answer: *mut NSView = unsafe { msg_send![super(self), hitTest: point] };
            let me: *const NSView = &**self;
            if std::ptr::eq(answer.cast_const(), me) {
                return std::ptr::null_mut();
            }
            answer
        }
    }
);

// ── the colours ────────────────────────────────────────────────────────────

/// **The straight-alpha reading of a premultiplied colour**, which is what a
/// `CGColor` is.
///
/// `Compositor::set_page_ground_color` takes its four numbers *premultiplied
/// and sRGB-encoded* because that is what the swapchain leaves in its own bytes
/// and the floor has to match it to the byte. A `CGColor` is not bytes: its
/// components are straight by definition and CoreAnimation multiplies them by
/// the alpha itself when it composites the layer. So the multiplication the
/// caller already did is undone here, once, rather than left to arrive on
/// screen twice — a 60 % ground would otherwise read at 36 %.
///
/// `a = 0` is the one case with no straight reading at all: nothing is being
/// asked for, and the honest answer is a colour that contributes nothing.
/// X-1 measured the other reading — an alpha-0 pixel carrying blue adds its
/// blue unattenuated — which is a fact about the *frame's* pixels and not about
/// a layer's background colour, and the reason this returns zeros rather than
/// dividing by zero.
#[must_use]
fn straight_srgb(premultiplied_srgb: [f32; 4]) -> [f64; 4] {
    let alpha = f64::from(premultiplied_srgb[3]).clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let straight = |channel: f32| (f64::from(channel) / alpha).clamp(0.0, 1.0);
    [
        straight(premultiplied_srgb[0]),
        straight(premultiplied_srgb[1]),
        straight(premultiplied_srgb[2]),
        alpha,
    ]
}

// ── the transaction every change is made in ────────────────────────────────

/// **One change to the layer tree, with nothing animated.**
///
/// CoreAnimation gives every animatable property of a layer-backed view a
/// default *action*, and a frame written outside a transaction runs it: a pane
/// that moved would slide to its new rectangle over a quarter of a second,
/// every frame it moved, while the frame Folio drew for it was already there.
/// `setDisableActions:` is the documented switch and a transaction is the only
/// scope it has, so the two are written together here and nowhere else.
///
/// The body is deliberately a value-returning closure with no `?` in it: an
/// early return between `begin` and `commit` would leave a transaction open on
/// the thread, and the way to make that unwritable is to have nothing to return
/// early from.
fn without_animation<T>(body: impl FnOnce() -> T) -> T {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    let answer = body();
    CATransaction::commit();
    answer
}

// ── the compositor ─────────────────────────────────────────────────────────

/// **The composition one window presents through, on macOS** (M4-1).
///
/// The same eleven doors as `windows_impl`'s `Compositor`, answering for a
/// window whose composition is a subview list rather than a visual tree. See
/// this module's own header for the arrangement and for the three alpha rules
/// it keeps.
///
/// # What is deliberately not here
///
/// **The skirt places nothing.** On Windows the two bands are the part of the
/// window the *swapchain* does not cover: a swapchain visual is exactly the
/// size of its buffer, a resize grows the window several milliseconds before
/// the buffer follows, and a tree bound `topmost = true` over a per-pixel-alpha
/// `HWND` shows the desktop in the difference. A `CAMetalLayer` is not sized to
/// its drawable — `raw-window-metal` keeps it on the **view**, which
/// autoresizes with the window — so there is no moment at which the layer is
/// smaller than the window, and CoreAnimation stretches the last drawable
/// across it instead. Measured twice: X-1 resized 1800×1200 → 1360×1500 with
/// every sample unchanged, and M1-4 dragged a real window's corner with the
/// frame unchanged (§13.14 ⑤). A band placed over a region that *is* being
/// painted would be the doubled translucency §7.1.6c-4b forbids, arriving on
/// the platform that did not have the defect.
///
/// So the two clocks and the pure question are kept exactly — the type tells
/// one story on both platforms, and [`Self::skirt_covers_anything`] still buys
/// the present that settles a resize — and what they drive is nothing.
pub struct Compositor {
    /// The window this composition is for. Held rather than re-derived so that
    /// every door asks the same handle the constructor was given, and so that
    /// the type has one shape on both platforms.
    window: NativeWindow,
    /// winit's own view: the content view every slot is a subview of, and the
    /// view whose `isFlipped` decides the arithmetic in [`Self::in_content`].
    content: Retained<NSView>,
    /// **Folio's own view, the one the `CAMetalLayer` hangs on** (M1-4).
    ///
    /// Found — or made — through `macos_impl`'s one door, so that this type and
    /// `bt-app`'s `window_surface_target` can never disagree about which view
    /// is Folio's. Held because [`Self::attach_web_visual`] has to order every
    /// slot *below* it by name rather than by a position in a list.
    surface: Retained<NSView>,
    /// **One slot per page**, each keyed by the [`PageVisual`] that asked for
    /// it — the shape `windows_impl` arrived at through W2 slice ③ and F1b′,
    /// and for the same reasons: a page is pointed at one host for its whole
    /// life, and seat numbers restart at one in every tab.
    ///
    /// `RefCell` because a web seat opens and closes while the window is
    /// running and every other door here takes `&self`.
    pages: RefCell<BTreeMap<PageVisual, Retained<PageSlot>>>,
    /// How big this window's client area is, as last reported. One of the
    /// skirt's two clocks — see [`Self::set_window_size`].
    window_size: Cell<(u32, u32)>,
    /// How much of it the picture covers, believed one present late. The
    /// skirt's other clock — see [`Self::set_covered_size`].
    covered_size: Cell<(u32, u32)>,
    /// The size the **last** present handed over, which is not yet the size on
    /// the glass.
    presented_size: Cell<(u32, u32)>,
    /// The colour every page's floor is painted, premultiplied and sRGB-encoded
    /// exactly as the caller hands it over. Kept so that a slot minted after
    /// the answer arrived is born wearing it, and so that a settings write that
    /// did not move it costs one comparison.
    ground_color: Cell<[f32; 4]>,
}

impl Compositor {
    /// Build the composition for one window. The window must already exist.
    ///
    /// **What it makes is Folio's own view**, through `macos_impl`'s one door —
    /// which is idempotent, so a window whose surface has already been built
    /// gets the view it already had. Step 13 of the sixteen-step startup path
    /// runs before step 15 asks for the same view, and the order does not
    /// matter to either.
    ///
    /// **Failure is failure**, exactly as the Windows arm's is: the two ways
    /// this can refuse are a thread that is not the window's and a handle whose
    /// window has gone, and neither is something a reader's machine can be.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        let (_mtm, content, surface) = folios_surface_view(window, "the window's composition")?;
        Ok(Self {
            window,
            content,
            surface,
            pages: RefCell::new(BTreeMap::new()),
            // Two zeroes and not the window's real size, for the Windows arm's
            // reason: the skirt is placed by whoever moves one of these, and
            // being born believing the window is empty is what makes the first
            // `set_window_size` a change.
            window_size: Cell::new((0, 0)),
            covered_size: Cell::new((0, 0)),
            presented_size: Cell::new((0, 0)),
            // Opaque black until the app says otherwise, which it does before
            // the window is ever shown. A floor that somehow reached the glass
            // ahead of that answer is still a floor and not a hole.
            ground_color: Cell::new([0.0, 0.0, 0.0, 1.0]),
        })
    }

    /// **Tell the composition how big the window is.**
    ///
    /// The first of the skirt's two clocks, and on this platform it moves
    /// nothing — see the type's own note for the measurement that says why.
    /// Cheap when nothing moved, which is what makes it safe to call from a
    /// handler that fires all through a drag.
    pub fn set_window_size(&self, width: u32, height: u32) -> Result<(), String> {
        if self.window_size.get() == (width, height) {
            return Ok(());
        }
        self.window_size.set((width, height));
        Ok(())
    }

    /// **Tell the composition how much of the window the picture covers, and
    /// believe it one present later.**
    ///
    /// The arithmetic is the Windows arm's to the line, including the lag: a
    /// present queues an image rather than showing one, so what the skirt is
    /// sized against is the picture of the present *before* this one. Kept
    /// identical rather than simplified away, because the question
    /// [`Self::skirt_covers_anything`] answers has to mean the same thing in
    /// both windows or the caller's loop means two things.
    pub fn set_covered_size(&self, width: u32, height: u32) -> Result<(), String> {
        let landed = self.presented_size.replace((width, height));
        if self.covered_size.get() == landed {
            return Ok(());
        }
        self.covered_size.set(landed);
        Ok(())
    }

    /// Whether the picture has caught up with the window yet.
    ///
    /// Read by the present funnel to ask for one more frame. Nothing is drawn
    /// for the region on this platform, and the answer still has to be the same
    /// answer: `false` ends the chain of presents that follows a resize, and a
    /// window that fell idle one frame early is a window whose last frame was
    /// drawn for the size before the last one.
    #[must_use]
    pub fn skirt_covers_anything(&self) -> bool {
        window_skirt(self.window_size.get(), self.covered_size.get())
            .iter()
            .any(|band| !band.is_empty())
    }

    /// **The view wgpu builds its surface upon** — the twin of the Windows
    /// arm's GPU visual.
    ///
    /// No ownership travels with it: `raw-window-metal` retains the view for as
    /// long as the layer it makes lives, and the view hierarchy retains it for
    /// as long as the window does, so the surface cannot outlive what it is
    /// drawn on even if this `Compositor` is dropped first.
    ///
    /// It cannot fail, and that is the reason the view is made in
    /// [`Self::new`]: a reader of a raw handle has nothing to do with a
    /// refusal, and the Windows arm's signature has no room for one.
    #[must_use]
    pub fn gpu_visual_ptr(&self) -> *mut c_void {
        NonNull::from(&*self.surface).as_ptr().cast()
    }

    /// Move Folio's own view inside the window, in **physical** pixels.
    ///
    /// The main window's is `(0, 0)` and stays there — the surface covers the
    /// whole content view. The door exists because a window whose picture is
    /// one sibling among several is the shape this arrangement was built for,
    /// and because putting the conversion in one place is what stops someone
    /// scaling a physical rectangle by a backing scale a second time (see
    /// [`composition_visual_offset`]).
    pub fn set_gpu_offset(&self, x: i32, y: i32) -> Result<(), String> {
        let what = "moving the window's picture";
        let _mtm = window_thread(what)?;
        let (x, y) = composition_visual_offset(x, y);
        let scale = self.backing_scale(what)?;
        let size = self.content.bounds().size;
        let frame = self.in_content(NSRect::new(
            NSPoint::new(f64::from(x) / scale, f64::from(y) / scale),
            size,
        ));
        without_animation(|| self.surface.setFrame(frame));
        Ok(())
    }

    /// **Give this page a slot, under everything Folio draws.** Idempotent: a
    /// second call for a page that already has one changes nothing.
    ///
    /// # Where it lands, and why that is the whole of this method
    ///
    /// The slot is added to the content view at [`VisualLayer::Bottom`], which
    /// [`ordering_for`] turns into `NSWindowBelow` with no reference view:
    /// behind every sibling already there, and therefore behind Folio's own
    /// surface view whenever that one was added — which it was, in
    /// [`Self::new`], before any page exists. So a page can only ever be under
    /// Folio's own painting, and a later slice that adds a third subview cannot
    /// get between them by forgetting something.
    ///
    /// Folio still has to *stop painting* where the page is — see
    /// `bt_render::WindowRenderer::set_web_holes`. This method decides who is
    /// on top; the hole decides where the top is transparent.
    ///
    /// **The floor comes with the slot** (§7.14). On Windows a page's floor is
    /// a second visual carrying one stretched texel of the window's ground; a
    /// slot is a view and a view has a layer, so the floor is that layer's
    /// background colour and the two cannot be placed apart because they are
    /// one object. A page whose engine has not arrived — the ordinary case for
    /// the first few hundred milliseconds — is therefore a rectangle of the
    /// window's own ground rather than a hole onto the desktop.
    pub fn attach_web_visual(&self, page: PageVisual) -> Result<(), String> {
        self.ensure_slot(page, "the page's slot")?;
        Ok(())
    }

    /// **Put M4-2's `WKWebView` into this page's slot**, and let it fill it.
    ///
    /// # Why this door exists at all, when Windows has no twin for it
    ///
    /// The two engines are hosted the opposite way round. WebView2 composes
    /// into a visual **the host makes**: `windows_impl` builds the page's
    /// visual and `Compositor::web_visual` hands it to
    /// `ICoreWebView2CompositionController` as its root visual target. WebKit
    /// makes its **own** view and the host has to take it: a `WKWebView` is an
    /// `NSView`, and the only thing to decide is where in the hierarchy it
    /// stands. So the handle travels in the other direction and needs a door of
    /// its own, and this is it.
    ///
    /// The handle is a [`NativeWindow`] for the reason that type exists: on
    /// this platform it is an `NSView` pointer already — it is what winit hands
    /// out for a window — and a second opaque wrapper for the same pointer
    /// would be a second thing a caller can get wrong.
    ///
    /// Idempotent, and it mints a slot for a page that has none: the page's
    /// engine and the page's rectangle arrive on two clocks, and neither is
    /// allowed to be the one that decides whether the other is possible.
    pub fn attach_page_view(&self, page: PageVisual, view: NativeWindow) -> Result<(), String> {
        let what = "hosting the page's own view";
        let _mtm = window_thread(what)?;
        let slot = self.ensure_slot(page, what)?;
        // SAFETY: the handle is a live `NSView` this process owns — M4-2 passes
        // the `WKWebView` it just made — and this is the window's own thread.
        let page_view: &NSView = unsafe { view.as_ns_view().as_ref() };
        // SAFETY: `superview` hands back a view it does not retain for the
        // caller, and the one it can hand back here is this window's own slot —
        // held by `self.pages` and by the content view's subview list, both of
        // which outlive this call.
        let standing_in = unsafe { page_view.superview() };
        if standing_in.is_some_and(|superview| std::ptr::eq(&*superview, &**slot)) {
            return Ok(());
        }
        without_animation(|| {
            page_view.setFrame(slot.bounds());
            page_view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            slot.addSubview_positioned_relativeTo(page_view, ordering_for(VisualLayer::Top), None);
        });
        Ok(())
    }

    /// **Put the window's ground colour under every page.**
    ///
    /// Takes it *premultiplied and sRGB-encoded* — four numbers in `0.0..=1.0`
    /// — because that is what the swapchain leaves in its own bytes and the two
    /// have to match to the byte or a pane reads as a slightly different shade
    /// of the same window. The arithmetic that produces them belongs to the
    /// renderer, which owns both the palette and the linear/sRGB boundary; this
    /// crate must not learn either. What it does own is the *representation*:
    /// see [`straight_srgb`] for why the numbers are divided by their own alpha
    /// on the way to a `CGColor`.
    ///
    /// Idempotent, and cheap when nothing moved.
    pub fn set_page_ground_color(&self, premultiplied_srgb: [f32; 4]) -> Result<(), String> {
        if self.ground_color.get() == premultiplied_srgb {
            return Ok(());
        }
        let _mtm = window_thread("the window's ground colour")?;
        self.ground_color.set(premultiplied_srgb);
        let slots = self.pages.borrow();
        without_animation(|| {
            for slot in slots.values() {
                paint_floor(slot, premultiplied_srgb);
            }
        });
        Ok(())
    }

    /// Move and crop this page's slot, in **physical** pixels.
    ///
    /// Two rectangles and not one, for the reason the Windows arm takes two:
    /// `offset` is where the page is laid out and `clip` is the box it may
    /// appear in, in the slot's own coordinates. At rest they are the same
    /// rectangle; mid-FLIP they are not. A `CALayer` has a clip of its own and
    /// an `NSView` does not need one — a subview is cropped by its frame — so
    /// the two arrive here and leave as **one** rectangle, which is what a slot
    /// is.
    ///
    /// **The slot is minted here if this is the first time this page has been
    /// put anywhere**, because the engine is not what a pane's rectangle waits
    /// on. Returning at all is this call's promise that a floor stands at
    /// `clip`, and the app cuts no hole without that promise
    /// (`bt_app::Runtime::hole_for`).
    pub fn place_web_visual(
        &self,
        page: PageVisual,
        offset: (i32, i32),
        clip: (f32, f32, f32, f32),
    ) -> Result<(), String> {
        let what = "placing the page";
        let _mtm = window_thread(what)?;
        let slot = self.ensure_slot(page, what)?;
        let scale = self.backing_scale(what)?;
        let (x, y) = composition_visual_offset(offset.0, offset.1);
        // `max(0)` rather than a refusal: a pane can legitimately be solved to
        // nothing on the frame a split collapses, and a negative size is a view
        // placed inside out.
        let width = f64::from((clip.2 - clip.0).max(0.0)) / scale;
        let height = f64::from((clip.3 - clip.1).max(0.0)) / scale;
        let frame = self.in_content(NSRect::new(
            NSPoint::new(
                (f64::from(x) + f64::from(clip.0)) / scale,
                (f64::from(y) + f64::from(clip.1)) / scale,
            ),
            NSSize::new(width, height),
        ));
        without_animation(|| slot.setFrame(frame));
        Ok(())
    }

    /// **Take this page's slot off the glass** — the symmetric door to
    /// [`Self::place_web_visual`], and the whole of what a page that is not on
    /// the glass leaves behind: nothing.
    ///
    /// An **empty** rectangle rather than an offscreen one, so that nothing
    /// about where the page last stood survives in the hierarchy — and an empty
    /// rectangle rather than a removal, because a page comes back onto the
    /// glass on the next tab switch and re-adding a subview would re-order it
    /// against every other page in the window, which is the one thing
    /// [`Self::attach_web_visual`] was written to make impossible.
    ///
    /// **It mints nothing.** A page nothing has ever placed has no slot to take
    /// down, and making one here would be this type building a view for a page
    /// that is not being shown.
    pub fn hide_web_visual(&self, page: PageVisual) -> Result<(), String> {
        let _mtm = window_thread("taking the page off the glass")?;
        let slots = self.pages.borrow();
        let Some(slot) = slots.get(&page) else {
            return Ok(());
        };
        without_animation(|| slot.setFrame(NSRect::ZERO));
        Ok(())
    }

    /// Take this page's slot out of the window.
    ///
    /// Called when the page goes away. Leaving an empty view behind would cost
    /// nothing on screen and would still be a lie in the hierarchy, which is
    /// what this file is for. The page's own view goes with it — a `WKWebView`
    /// inside a slot is a subview of the slot, and `removeFromSuperview` takes
    /// the pair.
    pub fn detach_web_visual(&self, page: PageVisual) -> Result<(), String> {
        let _mtm = window_thread("taking the page out of the window")?;
        let Some(slot) = self.pages.borrow_mut().remove(&page) else {
            return Ok(());
        };
        without_animation(|| slot.removeFromSuperview());
        Ok(())
    }

    /// Publish this frame. **Call once per presented frame**, as on Windows.
    ///
    /// # Why there is nothing to publish
    ///
    /// DirectComposition's tree is only on the glass once somebody calls
    /// `Commit`, and wgpu's dx12 backend deliberately does not — so on Windows
    /// this call is the whole of whether the picture ever moves again.
    /// CoreAnimation has no such handle: every layer change made on the main
    /// thread joins the **implicit transaction** the run loop opens for that
    /// turn and is committed when the turn ends, and every change this type
    /// makes is additionally inside an explicit transaction of its own
    /// ([`without_animation`]) which is committed in the call that opened it.
    ///
    /// So the door is kept and the body is empty, which is a different thing
    /// from the door being absent: the app's present funnel calls it in one
    /// place on both platforms, and the day something here needs a
    /// `CATransaction::flush` this is where it goes.
    pub fn commit(&self) -> Result<(), String> {
        Ok(())
    }

    /// This page's slot, made if this is the first anyone has asked.
    ///
    /// **One minting door**, exactly as the Windows arm has one: a page's slot
    /// is asked for by the engine arriving, by the pane being placed and by the
    /// page being hosted, and three creators would be three chances for a slot
    /// to end up in the wrong place in the list.
    fn ensure_slot(&self, page: PageVisual, what: &str) -> Result<Retained<PageSlot>, String> {
        let mtm = window_thread(what)?;
        if let Some(slot) = self.pages.borrow().get(&page) {
            return Ok(slot.clone());
        }
        let slot = self.make_slot(mtm);
        self.pages.borrow_mut().insert(page, slot.clone());
        Ok(slot)
    }

    /// One slot, born at no size and already carrying the window's ground.
    ///
    /// **At no size**, because a slot is placed by whoever placed the pane and
    /// a slot born at the content view's bounds would be a full-window slab of
    /// the ground colour standing over Folio's own picture for the frames
    /// between the two calls.
    fn make_slot(&self, mtm: MainThreadMarker) -> Retained<PageSlot> {
        let empty = NSRect::ZERO;
        // SAFETY: `NSView`'s designated initializer, on the main thread, on an
        // instance of a subclass that adds no ivars.
        let slot: Retained<PageSlot> =
            unsafe { msg_send![PageSlot::alloc(mtm), initWithFrame: empty] };
        without_animation(|| {
            // The floor is a layer's background colour, so the view has to have
            // a layer of its own rather than draw into its ancestor's — and the
            // layer is made here rather than left to AppKit, in the order
            // `NSView` documents (the layer first, `wantsLayer` after), so that
            // `layer()` has an answer from this line onwards rather than from
            // whenever the view is first displayed.
            slot.setLayer(Some(&CALayer::new()));
            slot.setWantsLayer(true);
            paint_floor(&slot, self.ground_color.get());
            self.content.addSubview_positioned_relativeTo(
                &slot,
                ordering_for(VisualLayer::Bottom),
                None,
            );
        });
        slot
    }

    /// The backing scale of the display this window is mostly on.
    ///
    /// Asked at the moment of use rather than stored, because it changes when
    /// the reader carries the window to another display and a stored one would
    /// place every pane at the old scale until something else noticed.
    fn backing_scale(&self, what: &str) -> Result<f64, String> {
        let (_mtm, ns_window) = window_for(self.window, what)?;
        Ok(ns_window.backingScaleFactor())
    }

    /// **One rectangle, from the space this program writes in to the content
    /// view's own** — the whole of the coordinate rule this file obeys.
    ///
    /// Everything `bt-layout` produces is in **physical pixels from the top-left
    /// of the client area**, and the caller has already divided by the backing
    /// scale by the time a rectangle reaches here, so what is left is the
    /// origin. winit's content view answers `isFlipped` **true** — its own
    /// comment says "winit uses the upper-left corner as the origin" — and for
    /// that view this function is the identity.
    ///
    /// It is still a function, and it still asks, because AppKit's default
    /// answer is the other one: a content view that is not winit's — the one a
    /// test builds, and any window this crate is ever handed from somewhere
    /// else — has its origin at the bottom-left, and a rectangle placed in it
    /// without the flip would be a pane at the bottom of a window whose layout
    /// put it at the top. Reading `isFlipped` is the documented question and
    /// the only correct one; assuming winit's answer would be a rule about who
    /// made the window rather than about what the window is.
    fn in_content(&self, rect: NSRect) -> NSRect {
        if self.content.isFlipped() {
            return rect;
        }
        let height = self.content.bounds().size.height;
        NSRect::new(
            NSPoint::new(rect.origin.x, height - rect.origin.y - rect.size.height),
            rect.size,
        )
    }
}

/// Paint one slot's floor.
///
/// A free function rather than a method on [`PageSlot`] because it is a
/// statement about the *ground*, which the compositor owns, rather than about
/// the view, which owns only its own hierarchy. The caller is inside a
/// transaction already — a background colour is as animatable as a frame.
fn paint_floor(slot: &PageSlot, premultiplied_srgb: [f32; 4]) {
    // `make_slot` gives every slot its layer before anything else touches it,
    // so there is no slot without one; the accessor answers an `Option` and a
    // quiet return is the only reading of it that is not a panic in the
    // platform layer.
    let Some(layer) = slot.layer() else {
        return;
    };
    let [red, green, blue, alpha] = straight_srgb(premultiplied_srgb);
    let colour = CGColor::new_srgb(red, green, blue, alpha);
    layer.setBackgroundColor(Some(&colour));
}

#[cfg(test)]
mod tests {
    use super::{ordering_for, straight_srgb};
    use crate::VisualLayer;
    use objc2_app_kit::NSWindowOrderingMode;

    /// **RED — the bottom of an AppKit subview list is `NSWindowBelow`, and the
    /// `bool` the Windows arm carries is not consulted.**
    ///
    /// The two lists run opposite ways. A DirectComposition child list is
    /// painted front to back, so its *beginning* is the bottom and
    /// `insertAbove = TRUE` with a NULL reference reaches it;
    /// `NSView.subviews` is back to front, so its *end* is the top and
    /// `NSWindowBelow` reaches the bottom. A reader who carried
    /// `insert_above_with_null_reference`'s `true` across and passed it to
    /// AppKit would be spelling `NSWindowAbove`, whose numeric value is `1` —
    /// which puts Folio's page on top of Folio's frame, the failure the W0′
    /// probe photographed on the other platform.
    ///
    /// Red gate: swap the two arms of `ordering_for`.
    #[test]
    fn the_bottom_of_a_subview_list_is_below_and_not_a_boolean() {
        assert_eq!(
            ordering_for(VisualLayer::Bottom),
            NSWindowOrderingMode::Below
        );
        assert_eq!(ordering_for(VisualLayer::Top), NSWindowOrderingMode::Above);
        // The trap, named as a number rather than described: the boolean the
        // Windows arm passes for `Bottom` is `true`, and `true` spelled as an
        // `NSWindowOrderingMode` is `Above` — the opposite end of the stack.
        assert!(VisualLayer::Bottom.insert_above_with_null_reference());
        assert_eq!(NSWindowOrderingMode::Above.0, 1);
        assert_eq!(NSWindowOrderingMode::Below.0, -1);
    }

    /// **RED — a premultiplied colour is un-multiplied exactly once on its way
    /// to a `CGColor`.**
    ///
    /// The caller hands over what the swapchain's own bytes carry — the colour
    /// already multiplied by its alpha — and CoreAnimation multiplies a
    /// `CGColor` by its alpha again when it composites. A floor that passed the
    /// numbers straight through would therefore land at `a²`: a 60 % ground
    /// reading at 36 %, which is the same kind of silent translucency defect
    /// §7.1.6c-4b names and is invisible in a screenshot unless somebody
    /// already knows the number.
    ///
    /// Red gate: return `premultiplied_srgb` unchanged.
    #[test]
    fn a_floors_colour_is_the_straight_reading_of_a_premultiplied_one() {
        // 60 % of white, as `bt-render` premultiplies it. The alpha is compared
        // against the `f32` widened rather than against the decimal it is
        // written as: the caller's numbers are `f32` and `f64::from(0.6_f32)`
        // is `0.600000023…`, which is the value and not a rounding error.
        let ground = straight_srgb([0.6, 0.6, 0.6, 0.6]);
        assert!((ground[0] - 1.0).abs() < 1e-9, "{ground:?}");
        assert!((ground[3] - f64::from(0.6_f32)).abs() < 1e-12, "{ground:?}");
        // An opaque colour is its own straight reading, which is the case every
        // window is in until somebody moves the slider.
        assert_eq!(
            straight_srgb([0.1, 0.2, 0.3, 1.0]),
            [
                f64::from(0.1_f32),
                f64::from(0.2_f32),
                f64::from(0.3_f32),
                1.0
            ]
        );
        // And nothing asked for is nothing painted, rather than a division by
        // zero.
        assert_eq!(straight_srgb([0.0, 0.0, 0.0, 0.0]), [0.0, 0.0, 0.0, 0.0]);
    }
}
