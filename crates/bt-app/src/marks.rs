//! The chrome's marks, taken from `design/ui-mockup.html` and rasterized.
//!
//! The mock-up draws every glyph in the window's own chrome as SVG: the caption
//! buttons are `<symbol id="i-gear|i-min|i-max|i-close">`, a tab and a terminal
//! pane head wear the session's profile mark (`#p-pwsh`), a preview head wears
//! `#i-file` and a files head `#i-folder`, both in `--accent`. Their bodies are
//! reproduced here verbatim, which is the whole point: a mark drawn twice — once
//! in the design and once in Rust — is two marks that drift, and the second one
//! is the one nobody looks at.
//!
//! The active tab's *shape* is here for the same reason and one more. CSS gives
//! it `border-radius: var(--tabr) var(--tabr) 0 0` plus two `::before`/`::after`
//! skirt corners filled with `--termbg`, and a browser renders all four with
//! analytic coverage. Approximating that with nested rectangles produces a
//! staircase, so the silhouette is emitted as one closed path and handed to the
//! same rasterizer, which antialiases it the way the design's own renderer does.
//!
//! Nothing in this module decides *where* a mark goes — `seats.rs` does, from the
//! solver's rectangles. This module answers only "what does it look like, at this
//! many physical pixels, in this colour".

use std::{collections::HashMap, fmt::Write as _, sync::Arc};

use bt_render::{ChromeIcon, ChromeLabel, OverlayQuad};

/// One mark the chrome can wear. Every variant except [`ChromeMark::ActiveTab`]
/// is a `<symbol>` lifted straight out of the mock-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeMark {
    /// `#i-gear` — the settings caption button.
    Gear,
    /// `#i-min`.
    WindowMinimize,
    /// `#i-max`.
    WindowMaximize,
    /// `#i-close`.
    WindowClose,
    /// The same `#i-close` source, at the tab control's smaller 8px size.
    TabClose,
    /// `#i-plus` — the new-tab button.
    Plus,
    /// `#i-chev` — the profile picker beside the `+`. The mock-up rotates the
    /// one arrow rather than swapping glyphs (`.chevbtn.open svg { transform:
    /// rotate(180deg) }`): it points down at a list that is folded away and up
    /// at one that is already on screen, and the turn is the sentence.
    Chevron { open: bool },
    /// `#p-pwsh` — the PowerShell profile mark, which carries its own colours.
    /// The mock-up is explicit that a mark's colour is its own and the active
    /// tab does not recolour it (`.pmark`, and the comment above it).
    ProfilePowerShell,
    /// `#i-file`.
    File,
    /// `#i-folder`.
    Folder,
    /// `#i-panel` — a pane whose kind this build cannot name.
    Panel,
    /// The active tab's silhouette: `--tabr` top corners and the two outward
    /// skirt corners that join it to the content plane. `radius_px` is `--tabr`
    /// in physical pixels, so the shape is generated at the size it is drawn.
    ActiveTab { radius_px: u32 },
    /// A regular tab's rounded body, used only for the inactive hover fill.
    TabBody { radius_px: u32 },
    /// A title-bar control's hover pill: one round on all four corners, filled
    /// edge to edge. `.newtab` wears it at 6px and `.tab .close` at 4px.
    ///
    /// It is a mark rather than a [`bt_render::ChromeQuad`] because a quad is a
    /// rectangle and this one is not: drawn as a quad the `+`'s highlight came
    /// out a hard-edged grey block sitting against the tab's own round, which is
    /// what this variant exists to delete. Going through the rasterizer gets the
    /// same analytic coverage the active tab's corners already get, instead of a
    /// staircase assembled from nested rectangles.
    ///
    /// Opaque, like every other chrome fill: the palette pre-composites each
    /// pill over the surface it lands on, because this pipeline blends in linear
    /// light and the design's does not — see `ChromePalette::tab_close_pill_on_content`.
    ControlPill { radius_px: u32 },
    /// One arc of the progress ring (`.pring`, mock-up lines 268-284).
    ///
    /// The mock-up's ring is a *pair* of concentric circles — a full-turn track
    /// under a partial arc, in two different colours — and a sprite carries one
    /// colour, so a ring on screen is two of these: a `sweep_milliturns: 1000`
    /// track and the arc over it, sharing a box. That is not a workaround but
    /// the better decomposition: every ring in the strip shares one cached track
    /// raster no matter what its arc is doing, and the arc is the only thing
    /// that has to be redrawn when the progress moves.
    ///
    /// Angles are thousandths of a turn clockwise from 12 o'clock. Turns rather
    /// than degrees because every angle this mark is ever given starts life as a
    /// fraction — a percentage, or a phase through a spin — and integers rather
    /// than a float because this is half of the raster cache's key, which has to
    /// hash and compare exactly.
    ProgressRing {
        start_milliturns: u16,
        sweep_milliturns: u16,
        /// `.pring circle { stroke-width: 2 }`, in physical pixels.
        stroke_px: u32,
    },
}

impl ChromeMark {
    fn id(self) -> &'static str {
        match self {
            Self::Gear => "i-gear",
            Self::WindowMinimize => "i-min",
            Self::WindowMaximize => "i-max",
            Self::WindowClose => "i-close",
            Self::TabClose => "tab-close",
            Self::Plus => "i-plus",
            Self::Chevron { open: false } => "i-chev",
            Self::Chevron { open: true } => "i-chev-open",
            Self::ProfilePowerShell => "p-pwsh",
            Self::File => "i-file",
            Self::Folder => "i-folder",
            Self::Panel => "i-panel",
            Self::ActiveTab { .. } => "tab",
            Self::TabBody { .. } => "tab-body",
            Self::ControlPill { .. } => "control-pill",
            Self::ProgressRing { .. } => "progress-ring",
        }
    }

    /// Whether `color` reaches this mark at all. A profile mark paints itself.
    fn takes_current_color(self) -> bool {
        self != Self::ProfilePowerShell
    }
}

/// A mark, the physical box it fills, and the colour `currentColor` resolves to.
///
/// Deliberately plain data with no pixels in it: the chrome builder is a pure
/// function of the solver's rectangles, and it stays testable — and cheap to run
/// on every pointer move — because deciding *what* to draw never rasterizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeSprite {
    pub mark: ChromeMark,
    /// `[left, top, right, bottom]`, in physical pixels of the whole surface.
    pub rect: [f32; 4],
    pub color: [u8; 3],
    /// A uniform fade over the whole mark, `0.0 ..= 1.0`.
    ///
    /// The mock-up asks for this twice and both times the artwork is unchanged
    /// and only its presence varies: `.ticon.working` breathes between 1 and
    /// .28, and `.ticon-wrap.dead .ticon` holds .35. It therefore rides to the
    /// renderer beside the pixels rather than into them — see
    /// [`ChromeIcon::opacity`] for why that distinction is load-bearing.
    pub opacity: f32,
    /// `filter: grayscale(1)` — draw the mark with its hue removed.
    ///
    /// Unlike [`Self::opacity`] this *is* baked into the raster and keyed with
    /// it, because it changes each pixel's colour rather than its coverage, and
    /// because the mark it exists for is the profile mark, which carries
    /// colours of its own that no palette entry can stand in for.
    pub grayscale: bool,
}

impl ChromeSprite {
    /// A mark at full strength in its own colours — what all but the tab's own
    /// state channels want.
    pub fn new(mark: ChromeMark, rect: [f32; 4], color: [u8; 3]) -> Self {
        Self {
            mark,
            rect,
            color,
            opacity: 1.0,
            grayscale: false,
        }
    }
}

/// One stacking layer of the modal overlay as a builder leaves it: its fills, its
/// captions, and its marks still named rather than rasterized.
///
/// The unrasterized twin of [`bt_render::OverlayLayer`], and it exists for the
/// same reason: the overlay's three channels have a fixed order inside the render
/// pass, so a popup is only above a row it covers if the two are on different
/// layers. Anything that must cover something else goes on a later layer; being
/// pushed later into the same layer buys nothing across channels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayLayer {
    pub quads: Vec<OverlayQuad>,
    pub labels: Vec<ChromeLabel>,
    pub sprites: Vec<ChromeSprite>,
}

impl OverlayLayer {
    /// Whether the layer draws nothing at all — an empty layer is not a layer,
    /// and handing one to the renderer would cost a text renderer and a pass
    /// through three channels to draw nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty() && self.labels.is_empty() && self.sprites.is_empty()
    }
}

/// Rasterized marks, keyed by mark + physical size + colour.
///
/// The map is rebuilt from the sprites of each frame, so it holds exactly what
/// is on screen: a DPI change, a theme change or a window narrow enough to
/// shorten the tab retires the old rasters on the next chrome rebuild instead of
/// accumulating one entry per size ever seen.
#[derive(Default)]
pub struct ChromeMarkRasters {
    rasters: HashMap<String, Raster>,
}

#[derive(Clone)]
struct Raster {
    rgba: Arc<[u8]>,
    width_px: u32,
    height_px: u32,
}

impl ChromeMarkRasters {
    /// Rasterize whatever is not already in hand and return the draw list, in
    /// the order the sprites were requested — which is the order they paint.
    pub fn resolve(&mut self, sprites: &[ChromeSprite]) -> Vec<ChromeIcon> {
        let mut kept: HashMap<String, Raster> = HashMap::with_capacity(sprites.len());
        let icons = self.icons_for(sprites, &mut kept);
        self.rasters = kept;
        icons
    }

    /// The same for a whole overlay stack, layer by layer.
    ///
    /// One pass over every layer rather than one call each, because the map is
    /// trimmed to what the *frame* asked for: resolving layer by layer would let
    /// the popup's marks retire the dialog's on the way past, and every frame
    /// would rasterize both again.
    pub fn resolve_overlay(&mut self, layers: Vec<OverlayLayer>) -> Vec<bt_render::OverlayLayer> {
        let mut kept: HashMap<String, Raster> = HashMap::new();
        let resolved = layers
            .into_iter()
            .map(|layer| bt_render::OverlayLayer {
                quads: layer.quads,
                labels: layer.labels,
                icons: self.icons_for(&layer.sprites, &mut kept),
            })
            .collect();
        self.rasters = kept;
        resolved
    }

    /// Rasterize what `kept` does not already hold, adding to it as it goes.
    fn icons_for(
        &self,
        sprites: &[ChromeSprite],
        kept: &mut HashMap<String, Raster>,
    ) -> Vec<ChromeIcon> {
        let mut icons = Vec::with_capacity(sprites.len());
        for sprite in sprites {
            let width_px = (sprite.rect[2] - sprite.rect[0]).round();
            let height_px = (sprite.rect[3] - sprite.rect[1]).round();
            if !(width_px >= 1.0 && height_px >= 1.0) {
                continue;
            }
            let (width_px, height_px) = (width_px as u32, height_px as u32);
            let key = mark_key(sprite, width_px, height_px);
            let raster = match kept.get(&key).or_else(|| self.rasters.get(&key)) {
                Some(raster) => raster.clone(),
                None => {
                    let Some(raster) = rasterize(sprite, width_px, height_px) else {
                        continue;
                    };
                    raster
                }
            };
            icons.push(ChromeIcon {
                key: key.clone(),
                rect: sprite.rect,
                rgba: Arc::clone(&raster.rgba),
                width_px: raster.width_px,
                height_px: raster.height_px,
                opacity: sprite.opacity,
            });
            kept.insert(key, raster);
        }
        icons
    }
}

fn mark_key(sprite: &ChromeSprite, width_px: u32, height_px: u32) -> String {
    let [r, g, b] = sprite.color;
    let mut key = format!("chrome-mark:{}", sprite.mark.id());
    if let ChromeMark::ActiveTab { radius_px } | ChromeMark::TabBody { radius_px } = sprite.mark {
        let _ = write!(key, ":r{radius_px}");
    }
    if let ChromeMark::ControlPill { radius_px } = sprite.mark {
        let _ = write!(key, ":r{radius_px}");
    }
    if let ChromeMark::ProgressRing {
        start_milliturns,
        sweep_milliturns,
        stroke_px,
    } = sprite.mark
    {
        let _ = write!(key, ":a{start_milliturns}+{sweep_milliturns}w{stroke_px}");
    }
    let _ = write!(key, ":{width_px}x{height_px}");
    if sprite.mark.takes_current_color() {
        let _ = write!(key, ":{r:02x}{g:02x}{b:02x}");
    }
    // Desaturation is in the pixels, so it has to be in the identity of the
    // pixels. Opacity deliberately is not — it never touches the raster.
    if sprite.grayscale {
        key.push_str(":grey");
    }
    key
}

fn rasterize(sprite: &ChromeSprite, width_px: u32, height_px: u32) -> Option<Raster> {
    let document = svg_document(sprite, width_px, height_px)?;
    let raster = bt_math::rasterize_svg_document(document.as_bytes()).ok()?;
    let mut rgba = raster.rgba;
    if sprite.grayscale {
        desaturate(&mut rgba);
    }
    Some(Raster {
        rgba: Arc::from(rgba),
        width_px: raster.width_px,
        height_px: raster.height_px,
    })
}

/// `filter: grayscale(1)` over straight sRGB RGBA, in place.
///
/// The coefficients are the ones CSS names for this filter: the
/// `feColorMatrix` `type="saturate" values="0"` matrix of the Filter Effects
/// spec, which is Rec. 709 luma applied to the sRGB values as they are stored.
/// A browser applies it in exactly that space, so converting to linear light
/// first would be a *different* grey than the one the design asks for.
///
/// Alpha is untouched: desaturating changes what colour a pixel is, never
/// whether it is there — the mark's silhouette and its antialiasing survive.
fn desaturate(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let luma = 0.2126 * f32::from(pixel[0])
            + 0.7152 * f32::from(pixel[1])
            + 0.0722 * f32::from(pixel[2]);
        let grey = luma.round().clamp(0.0, 255.0) as u8;
        pixel[0] = grey;
        pixel[1] = grey;
        pixel[2] = grey;
    }
}

/// A standalone document at exactly the physical size the box wants. The
/// `viewBox` is the symbol's own coordinate system, so the mark is scaled by the
/// renderer's own transform rather than by pre-multiplied path data — which is
/// what keeps one source of truth for the geometry across every DPI.
fn svg_document(sprite: &ChromeSprite, width_px: u32, height_px: u32) -> Option<String> {
    let (view_box, body) = match sprite.mark {
        ChromeMark::ActiveTab { radius_px } => {
            let path = active_tab_path(width_px, height_px, radius_px)?;
            (
                format!("0 0 {width_px} {height_px}"),
                format!(r#"<path fill="currentColor" d="{path}"/>"#),
            )
        }
        ChromeMark::TabBody { radius_px } => {
            let path = tab_body_path(width_px, height_px, radius_px)?;
            (
                format!("0 0 {width_px} {height_px}"),
                format!(r#"<path fill="currentColor" d="{path}"/>"#),
            )
        }
        ChromeMark::ControlPill { radius_px } => {
            let path = control_pill_path(width_px, height_px, radius_px)?;
            (
                format!("0 0 {width_px} {height_px}"),
                format!(r#"<path fill="currentColor" d="{path}"/>"#),
            )
        }
        ChromeMark::ProgressRing {
            start_milliturns,
            sweep_milliturns,
            stroke_px,
        } => (
            format!("0 0 {width_px} {height_px}"),
            progress_ring_body(
                width_px,
                height_px,
                stroke_px,
                start_milliturns,
                sweep_milliturns,
            )?,
        ),
        // One arrow that turns over: the `open` chevron is the resting one
        // rotated about its own centre, exactly as `.chevbtn.open svg` is.
        ChromeMark::Chevron { open: true } => (
            SYMBOL_VIEW_BOX[symbol_index(sprite.mark)].to_owned(),
            format!(
                r#"<g transform="rotate(180 5 3)">{}</g>"#,
                SYMBOL_BODY[symbol_index(sprite.mark)]
            ),
        ),
        mark => (SYMBOL_VIEW_BOX[symbol_index(mark)].to_owned(), {
            SYMBOL_BODY[symbol_index(mark)].to_owned()
        }),
    };
    let [r, g, b] = sprite.color;
    let body = if sprite.mark.takes_current_color() {
        body.replace("currentColor", &format!("#{r:02x}{g:02x}{b:02x}"))
    } else {
        body
    };
    Some(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width_px}" height="{height_px}" viewBox="{view_box}">{body}</svg>"#
    ))
}

/// One arc of `.pring`, as the mock-up draws it: a stroked `<circle>` with a
/// dash pattern selecting the live part of it.
///
/// Every decision here is quoted rather than invented:
///
/// * `fill: none; stroke-width: 2` and `stroke-linecap: round` are `.pring
///   circle` and `.pring .arc` (mock-up lines 277-279).
/// * the dash pattern is how the mock-up itself states an arc — `stroke-dasharray`
///   against `PRING_C`, with `stroke-dashoffset` walking it (line 4118-4130) —
///   rather than a hand-built elliptical-arc path, which would have to
///   special-case the full turn that a dash pattern gets for free.
/// * `rotate(-90 …)` is `.pring { transform: … rotate(-90deg) }` (line 273): an
///   SVG circle's own path begins at 3 o'clock, and progress begins at 12.
///
/// The radius is the box's half-width less half the stroke, so the stroke —
/// which straddles its path — lands exactly inside the box and clips nowhere.
fn progress_ring_body(
    width_px: u32,
    height_px: u32,
    stroke_px: u32,
    start_milliturns: u16,
    sweep_milliturns: u16,
) -> Option<String> {
    let side = width_px.min(height_px) as f32;
    let stroke = stroke_px as f32;
    let radius = (side - stroke) / 2.0;
    // Below this the ring is not a ring: the hole has closed and what is left is
    // a blob. A degenerate ring would be a second, unruled shape.
    if stroke < 1.0 || radius <= stroke / 2.0 {
        return None;
    }
    let sweep = f32::from(sweep_milliturns.min(1000)) / 1000.0;
    if sweep <= 0.0 {
        // Not an error: a determinate ring at 0% is a track and no arc, and the
        // caller draws the track as its own sprite.
        return Some(String::new());
    }
    let (centre_x, centre_y) = (width_px as f32 / 2.0, height_px as f32 / 2.0);
    let circumference = std::f32::consts::TAU * radius;
    let dash = circumference * sweep;
    let start = circumference * f32::from(start_milliturns % 1000) / 1000.0;
    // A round cap adds half a stroke beyond each end of the dash. On a full turn
    // that overshoot would wrap the two caps past one another; on a partial arc
    // it is the mock-up's own `stroke-linecap: round`.
    let linecap = if sweep >= 1.0 { "butt" } else { "round" };
    Some(format!(
        r#"<circle cx="{centre_x}" cy="{centre_y}" r="{radius}" fill="none" stroke="currentColor" stroke-width="{stroke}" stroke-linecap="{linecap}" stroke-dasharray="{dash} {circumference}" stroke-dashoffset="{offset}" transform="rotate(-90 {centre_x} {centre_y})"/>"#,
        offset = -start,
    ))
}

fn symbol_index(mark: ChromeMark) -> usize {
    match mark {
        ChromeMark::Gear => 0,
        ChromeMark::WindowMinimize => 1,
        ChromeMark::WindowMaximize => 2,
        ChromeMark::WindowClose => 3,
        ChromeMark::TabClose => 3,
        ChromeMark::Plus => 4,
        ChromeMark::ProfilePowerShell => 5,
        ChromeMark::File => 6,
        ChromeMark::Folder => 7,
        ChromeMark::Panel => 8,
        ChromeMark::Chevron { .. } => 9,
        // Handled before this function is reached; their geometry is generated,
        // not quoted.
        ChromeMark::ActiveTab { .. } => 8,
        ChromeMark::TabBody { .. } => 8,
        ChromeMark::ControlPill { .. } => 8,
        ChromeMark::ProgressRing { .. } => 8,
    }
}

const SYMBOL_VIEW_BOX: [&str; 10] = [
    "0 0 24 24",
    "0 0 10 10",
    "0 0 10 10",
    "0 0 10 10",
    "0 0 10 10",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 10 6",
];

/// The `<symbol>` bodies, byte for byte from `design/ui-mockup.html` (the
/// `<svg style="display:none">` block near the top of `<body>`).
const SYMBOL_BODY: [&str; 10] = [
    // #i-gear
    r#"<path fill="currentColor" d="M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.488.488 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.484.484 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z"/>"#,
    // #i-min
    r#"<path d="M0 5h10" fill="none" stroke="currentColor" stroke-width="1"/>"#,
    // #i-max
    r#"<rect x="0.5" y="0.5" width="9" height="9" rx="1.8" fill="none" stroke="currentColor" stroke-width="1"/>"#,
    // #i-close
    r#"<path d="M0.5 0.5l9 9M9.5 0.5l-9 9" fill="none" stroke="currentColor" stroke-width="1"/>"#,
    // #i-plus. `fill="none"` is ours: the mock-up's own path carries no fill
    // attribute and therefore fills black, which costs a browser nothing because
    // two straight subpaths enclose no area — but it is a lie about the glyph,
    // and this rasterizer has no reason to be handed one.
    r#"<path d="M5 0.5v9M0.5 5h9" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#,
    // #p-pwsh — flat, and its own colours (a mark carries its own).
    concat!(
        r##"<rect x="1" y="2.5" width="14" height="11" rx="1.8" fill="#2C5C9E"/>"##,
        r##"<path d="M4.4 5.7L7.3 8l-2.9 2.3" fill="none" stroke="#fff" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"/>"##,
        r##"<path d="M8.5 10.9h3.2" stroke="#fff" stroke-width="1.35" stroke-linecap="round"/>"##,
    ),
    // #i-file
    concat!(
        r#"<path d="M3.5 1.8h5.2l3.8 3.8v8.6c0 .3-.2.5-.5.5H3.5c-.3 0-.5-.2-.5-.5V2.3c0-.3.2-.5.5-.5z" fill="none" stroke="currentColor" stroke-width="1.15"/>"#,
        r#"<path d="M8.6 1.9v3.8h3.8" fill="none" stroke="currentColor" stroke-width="1.15" stroke-linejoin="round"/>"#,
    ),
    // #i-folder
    r#"<path d="M1.6 4.2c0-.6.5-1.1 1.1-1.1h3.1l1.3 1.5h6.2c.6 0 1.1.5 1.1 1.1v6.6c0 .6-.5 1.1-1.1 1.1H2.7c-.6 0-1.1-.5-1.1-1.1z" fill="currentColor"/>"#,
    // #i-panel
    concat!(
        r#"<rect x="1.5" y="2.5" width="13" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.1"/>"#,
        r#"<path d="M6.2 2.5v11" stroke="currentColor" stroke-width="1.1"/>"#,
    ),
    // #i-chev
    r#"<path d="M1 1l4 3.6L9 1" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#,
];

/// The active tab's closed outline, in physical pixels, clockwise from the
/// bottom-left of the left skirt.
///
/// `width` spans the tab *and* both skirts, so the tab body itself is
/// `width - 2 * radius` wide — which is exactly how the mock-up lays it out: the
/// strip's `padding-left: var(--tabr)` puts the first tab one corner in, and
/// `.tab.active::before` fills that corner, so the silhouette starts at the
/// window's own left edge.
///
/// Four arcs, two of each kind:
///
/// * the two top corners are `border-radius: var(--tabr) var(--tabr) 0 0` —
///   convex, sweeping in the positive direction;
/// * the two skirt corners are the `radial-gradient(circle var(--tabr) at 0 0,
///   transparent 98%, var(--termbg) 100%)` pair — concave, centred *outside* the
///   silhouette, which is what makes the tab flare into the content plane
///   instead of ending in a hard corner.
///
/// The bottom edge is the closing straight line at `y = height`: the tab's fill
/// is `--termbg` and so is the surface directly below it, so there is nothing to
/// draw there and nothing may be drawn there.
fn active_tab_path(width: u32, height: u32, radius: u32) -> Option<String> {
    let (w, h, r) = (width as i64, height as i64, radius as i64);
    // Four radii across and two down: below that the shape is not a rounded tab
    // with skirts any more, and inventing a degenerate one would be a second,
    // unruled silhouette.
    if r < 1 || w < 4 * r || h < 2 * r {
        return None;
    }
    Some(format!(
        "M0,{h} \
         A{r},{r} 0 0 0 {r},{skirt_top} \
         L{r},{r} \
         A{r},{r} 0 0 1 {two_r},0 \
         L{top_right},0 \
         A{r},{r} 0 0 1 {body_right},{r} \
         L{body_right},{skirt_top} \
         A{r},{r} 0 0 0 {w},{h} Z",
        skirt_top = h - r,
        two_r = 2 * r,
        top_right = w - 2 * r,
        body_right = w - r,
    ))
}

/// A `border-radius: Npx` box, all four corners, in physical pixels.
///
/// The radius is clamped to half the shorter side the way a browser clamps it,
/// so a pill asked for more round than it has room for becomes a stadium rather
/// than a self-intersecting path.
fn control_pill_path(width: u32, height: u32, radius: u32) -> Option<String> {
    let (w, h) = (width as i64, height as i64);
    let r = (radius as i64).min(w / 2).min(h / 2);
    if r < 1 || w < 2 || h < 2 {
        return None;
    }
    Some(format!(
        "M{r},0 L{right},0 A{r},{r} 0 0 1 {w},{r} \
         L{w},{bottom} A{r},{r} 0 0 1 {right},{h} \
         L{r},{h} A{r},{r} 0 0 1 0,{bottom} \
         L0,{r} A{r},{r} 0 0 1 {r},0 Z",
        right = w - r,
        bottom = h - r,
    ))
}

fn tab_body_path(width: u32, height: u32, radius: u32) -> Option<String> {
    let (w, h, r) = (width as i64, height as i64, radius as i64);
    if r < 1 || w < 2 * r || h < r {
        return None;
    }
    Some(format!(
        "M0,{h} L0,{r} A{r},{r} 0 0 1 {r},0 L{right},0 A{r},{r} 0 0 1 {w},{r} L{w},{h} Z",
        right = w - r,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite(mark: ChromeMark, width: f32, height: f32, color: [u8; 3]) -> ChromeSprite {
        ChromeSprite::new(mark, [0.0, 0.0, width, height], color)
    }

    fn alpha_at(icon: &ChromeIcon, x: u32, y: u32) -> u8 {
        icon.rgba[((y * icon.width_px + x) * 4 + 3) as usize]
    }

    fn rgb_at(icon: &ChromeIcon, x: u32, y: u32) -> [u8; 3] {
        let base = ((y * icon.width_px + x) * 4) as usize;
        [icon.rgba[base], icon.rgba[base + 1], icon.rgba[base + 2]]
    }

    /// A point on the ring's own stroke centreline, `turns` clockwise from 12.
    fn on_ring(icon: &ChromeIcon, turns: f32, stroke_px: f32) -> (u32, u32) {
        let centre = icon.width_px as f32 / 2.0;
        let radius = (icon.width_px as f32 - stroke_px) / 2.0;
        let angle = turns * std::f32::consts::TAU;
        (
            (centre + radius * angle.sin()).round() as u32,
            (centre - radius * angle.cos()).round() as u32,
        )
    }

    fn ring(sweep_milliturns: u16, stroke_px: u32, side: f32) -> ChromeSprite {
        sprite(
            ChromeMark::ProgressRing {
                start_milliturns: 0,
                sweep_milliturns,
                stroke_px,
            },
            side,
            side,
            [0x82, 0x8f, 0xff],
        )
    }

    /// PIN (T2 progress ring): the arc starts at 12 o'clock and runs clockwise.
    ///
    /// The mock-up gets both for free — its `.pring` is `rotate(-90deg)` over an
    /// SVG `<circle>`, whose path a browser walks clockwise from 3 o'clock — and
    /// so the design never had to say either out loud. Generated geometry has
    /// to, because every one of the ways to get this wrong (starting at 3
    /// o'clock, running anticlockwise, or both) still draws a perfectly
    /// plausible ring, and only a progress report that runs backwards tells you.
    #[test]
    fn the_progress_arc_starts_at_noon_and_sweeps_clockwise() {
        let stroke = 4.0_f32;
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[ring(250, stroke as u32, 30.0)]);
        let icon = &icons[0];
        // A quarter turn covers noon through 3 o'clock and nothing else.
        for turns in [0.02_f32, 0.10, 0.15, 0.23] {
            let (x, y) = on_ring(icon, turns, stroke);
            assert!(
                alpha_at(icon, x, y) > 128,
                "a quarter sweep must ink {turns} turns from noon, at ({x},{y})"
            );
        }
        for turns in [0.35_f32, 0.5, 0.75, 0.95] {
            let (x, y) = on_ring(icon, turns, stroke);
            assert_eq!(
                alpha_at(icon, x, y),
                0,
                "a quarter sweep must leave {turns} turns from noon bare, at ({x},{y})"
            );
        }
    }

    /// PIN (T2 progress ring): a longer sweep is always more ink.
    ///
    /// A ring that saturates — drawing the same arc for 40% as for 90% — is the
    /// failure that looks perfect in a screenshot and is useless in motion.
    #[test]
    fn a_longer_sweep_is_always_more_ink() {
        let mut inked = Vec::new();
        for sweep in [0_u16, 125, 250, 500, 750, 1000] {
            let mut rasters = ChromeMarkRasters::default();
            let icons = rasters.resolve(&[ring(sweep, 4, 30.0)]);
            inked.push(
                icons
                    .first()
                    .map(|icon| {
                        icon.rgba
                            .chunks_exact(4)
                            .map(|pixel| u32::from(pixel[3]))
                            .sum::<u32>()
                    })
                    .unwrap_or_default(),
            );
        }
        assert_eq!(inked[0], 0, "a zero sweep draws no arc at all");
        for window in inked.windows(2) {
            assert!(
                window[1] > window[0],
                "sweep coverage must be monotonic, got {inked:?}"
            );
        }
    }

    /// PIN (T2 progress ring): the ring fills the slot it replaces, clips
    /// nowhere, and stays hollow.
    ///
    /// The ring takes the mark's own box (user ruling), so its stroke has to
    /// live entirely inside it: a stroke straddles its path, and a radius
    /// chosen as if it did not shaves the ring flat against all four edges.
    #[test]
    fn the_ring_fills_its_slot_without_clipping_and_stays_hollow() {
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[ring(1000, 4, 30.0)]);
        let icon = &icons[0];
        let centre = icon.width_px / 2;
        // Noon, 3, 6 and 9 o'clock each reach the box's own edge.
        assert!(
            alpha_at(icon, centre, 0) > 0,
            "the ring is clipped at the top"
        );
        assert!(
            alpha_at(icon, centre, icon.height_px - 1) > 0,
            "the ring is clipped at the bottom"
        );
        assert!(
            alpha_at(icon, 0, centre) > 0,
            "the ring is clipped at the left"
        );
        assert!(
            alpha_at(icon, icon.width_px - 1, centre) > 0,
            "the ring is clipped at the right"
        );
        assert_eq!(
            alpha_at(icon, centre, centre),
            0,
            "the middle of a ring is a hole, not a fill"
        );
        assert_eq!(alpha_at(icon, 0, 0), 0, "a ring has no corners");
    }

    /// PIN (T2 unread dot): the dot is a circle, and the chrome already had a
    /// primitive for it.
    ///
    /// `.unreaddot { width: 6px; height: 6px; border-radius: 50% }` is a square
    /// with its round set to half its side, which is exactly what `ControlPill`
    /// clamps to — so the dot needs no shape of its own, and giving it one
    /// would be a second circle to keep in step with the first.
    #[test]
    fn the_unread_dot_is_a_control_pill_rounded_into_a_circle() {
        let accent = [0x82, 0x8f, 0xff];
        let side = 12.0_f32;
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[sprite(
            ChromeMark::ControlPill {
                radius_px: side as u32 / 2,
            },
            side,
            side,
            accent,
        )]);
        let icon = &icons[0];
        let centre = icon.width_px / 2;
        assert_eq!(alpha_at(icon, centre, centre), 255, "the dot is filled");
        assert_eq!(
            rgb_at(icon, centre, centre),
            accent,
            "the dot wears its claim"
        );
        for (x, y) in [(0, 0), (icon.width_px - 1, 0), (0, icon.height_px - 1)] {
            assert_eq!(
                alpha_at(icon, x, y),
                0,
                "a circle has no corner at ({x},{y})"
            );
        }
        assert!(
            alpha_at(icon, centre, 0) > 0,
            "the dot must reach its own top"
        );
        assert!(
            alpha_at(icon, 0, centre) > 0,
            "the dot must reach its own left"
        );
    }

    /// PIN (T2 dead marks): `filter: grayscale(1)` (mock-up line 285) is baked
    /// into the raster, and it is part of the mark's content identity.
    ///
    /// It has to be baked, unlike opacity: desaturating changes which colour
    /// each pixel is, and the mark this lands on is the profile mark, which
    /// carries colours of its own that no palette entry can stand in for.
    /// Because it changes the pixels it must also change the key — two rasters
    /// that differ while sharing a key is the one cache bug that shows up as
    /// the wrong picture.
    #[test]
    fn a_dead_mark_is_desaturated_in_the_raster_and_keyed_apart() {
        let accent = [0x82, 0x8f, 0xff];
        let live = sprite(ChromeMark::ProfilePowerShell, 30.0, 30.0, accent);
        let mut dead = live;
        dead.grayscale = true;
        dead.rect = [40.0, 0.0, 70.0, 30.0];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[live, dead]);
        assert_eq!(
            icons.len(),
            2,
            "two marks, not one shared by a colliding key"
        );
        assert_ne!(
            icons[0].key, icons[1].key,
            "a desaturated mark is different pixels and needs a different key"
        );
        // The PowerShell mark's panel is a saturated blue; dead, it must be grey.
        let (x, y) = (icons[0].width_px / 5, icons[0].height_px / 2);
        let [r, g, b] = rgb_at(&icons[0], x, y);
        assert!(
            u32::from(b) > u32::from(r) + 24,
            "the living mark is blue ({r},{g},{b})"
        );
        let [r, g, b] = rgb_at(&icons[1], x, y);
        let spread = u32::from(r.max(g).max(b)) - u32::from(r.min(g).min(b));
        assert!(spread <= 2, "a dead mark keeps no hue, got ({r},{g},{b})");
    }

    /// PIN (T2 breathing): a sprite's opacity rides to the renderer on the icon
    /// and never into the cache key.
    ///
    /// Two marks differing only in how faded they are must be the *same* raster,
    /// or a 1.7s breath mints a fresh texture on every frame it is drawn. This
    /// is the assertion that keeps the breath free.
    #[test]
    fn opacity_travels_on_the_icon_and_never_into_the_cache_key() {
        let accent = [0x82, 0x8f, 0xff];
        let full = sprite(ChromeMark::Folder, 26.0, 26.0, accent);
        let mut faded = full;
        faded.opacity = 0.28;
        faded.rect = [40.0, 0.0, 66.0, 26.0];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[full, faded]);
        assert_eq!(icons[0].key, icons[1].key, "one mark, one raster");
        assert!(
            Arc::ptr_eq(&icons[0].rgba, &icons[1].rgba),
            "the faded mark must reuse the pixels, not re-rasterize them"
        );
        assert_eq!(icons[0].opacity, 1.0);
        assert_eq!(icons[1].opacity, 0.28);
        // The raster itself is untouched by the fade — the alpha the renderer
        // multiplies is still the mark's own coverage.
        assert_eq!(
            alpha_at(&icons[1], icons[1].width_px / 2, icons[1].height_px * 2 / 3),
            255
        );
    }

    /// PIN (visual fidelity pass): every mark the chrome wears is a real raster
    /// with ink in it, at exactly the physical box asked for, and a mark that
    /// takes `currentColor` wears the colour it was handed.
    ///
    /// Red gate: a placeholder block would be opaque *everywhere*, so each mark
    /// is also required to leave transparent pixels — that is what tells a glyph
    /// apart from the filled square this pass exists to delete.
    #[test]
    fn every_chrome_mark_rasterizes_to_its_box_with_ink_and_negative_space() {
        let accent = [0x82, 0x8f, 0xff];
        let cases = [
            (ChromeMark::Gear, 14.0_f32),
            (ChromeMark::WindowMinimize, 10.0),
            (ChromeMark::WindowMaximize, 10.0),
            (ChromeMark::WindowClose, 10.0),
            (ChromeMark::TabClose, 8.0),
            (ChromeMark::Plus, 10.0),
            (ChromeMark::ProfilePowerShell, 15.0),
            (ChromeMark::File, 14.0),
            (ChromeMark::Folder, 13.0),
            (ChromeMark::Panel, 13.0),
        ];
        for (mark, size) in cases {
            for scale in [1.0_f32, 1.5, 2.0] {
                let side = (size * scale).round();
                let mut rasters = ChromeMarkRasters::default();
                let icons = rasters.resolve(&[sprite(mark, side, side, accent)]);
                let icon = icons
                    .first()
                    .unwrap_or_else(|| panic!("{mark:?} must rasterize at {side}px"));
                assert_eq!(
                    (icon.width_px, icon.height_px),
                    (side as u32, side as u32),
                    "{mark:?} must fill exactly the box it was given"
                );
                assert_eq!(
                    icon.rgba.len(),
                    (side as usize) * (side as usize) * 4,
                    "{mark:?} raster is not its own dimensions"
                );
                let inked = icon.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
                let clear = icon.rgba.chunks_exact(4).filter(|p| p[3] == 0).count();
                let strongest = icon
                    .rgba
                    .chunks_exact(4)
                    .map(|p| p[3])
                    .max()
                    .unwrap_or_default();
                assert!(inked > 0, "{mark:?} at {side}px drew nothing");
                // A one-pixel hairline centred on a pixel boundary splits its
                // coverage over two rows, so the floor is "clearly visible",
                // not "opaque" — but a mark that is all haze is still a bug.
                assert!(
                    strongest >= 100,
                    "{mark:?} at {side}px is too faint to read ({strongest})"
                );
                assert!(
                    clear > 0,
                    "{mark:?} at {side}px is a solid block — the placeholder this pass removes"
                );
            }
        }
    }

    /// PIN: `currentColor` marks answer to the palette, and a profile mark does
    /// not — the mock-up rules that a mark carries its own colour (`.pmark`).
    #[test]
    fn current_color_marks_take_the_palette_and_a_profile_mark_keeps_its_own() {
        let accent = [0x82, 0x8f, 0xff];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::Folder, 26.0, 26.0, accent),
            sprite(ChromeMark::ProfilePowerShell, 30.0, 30.0, accent),
        ]);
        let folder = &icons[0];
        // Dead centre of a filled folder body is solid accent.
        let centre = rgb_at(folder, folder.width_px / 2, folder.height_px * 2 / 3);
        assert_eq!(
            alpha_at(folder, folder.width_px / 2, folder.height_px * 2 / 3),
            255
        );
        assert_eq!(centre, accent, "a folder mark is drawn in --accent");

        let pwsh = &icons[1];
        let panel = rgb_at(pwsh, pwsh.width_px / 5, pwsh.height_px / 2);
        assert_ne!(panel, accent, "a profile mark is never recoloured");
        assert_eq!(
            panel,
            [0x2c, 0x5c, 0x9e],
            "the PowerShell panel keeps the mock-up's own #2C5C9E"
        );
    }

    /// PIN (tab shape): the active tab is round on top, square-cut at the
    /// bottom, and flares outward into the content plane at both lower corners.
    ///
    /// Every claim is read off the raster rather than off the path string:
    ///
    /// * the top-left corner pixel is empty and the pixel a radius in is full —
    ///   that is the `--tabr` round;
    /// * the bottom row is solid across the tab body *and* keeps going past both
    ///   of its sides into the skirts, so the tab and the surface below it are
    ///   one continuous field of `--termbg`;
    /// * the pixel at the skirt's own circle centre is *empty* — the flare is
    ///   concave, which is what a nested-rectangle staircase can never be, and
    ///   what distinguishes this from a plain rounded rectangle.
    #[test]
    fn the_active_tab_silhouette_is_round_on_top_and_flares_into_the_content_plane() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let radius = (7.0 * scale).round() as u32;
            let height = (34.0 * scale).round() as u32;
            let width = (200.0 * scale).round() as u32 + 2 * radius;
            let mut rasters = ChromeMarkRasters::default();
            let icons = rasters.resolve(&[ChromeSprite::new(
                ChromeMark::ActiveTab { radius_px: radius },
                [0.0, 0.0, width as f32, height as f32],
                [0x1b, 0x1b, 0x1b],
            )]);
            let tab = icons.first().expect("the tab silhouette must rasterize");
            let last_row = height - 1;

            // Round on top: the tab body starts at x = radius, and its own
            // top-left corner is cut away.
            assert_eq!(
                alpha_at(tab, radius, 0),
                0,
                "scale {scale}: the tab's top-left corner pixel must be outside the round"
            );
            assert_eq!(
                alpha_at(tab, 2 * radius, 0),
                255,
                "scale {scale}: one radius in, the top edge is solid"
            );
            assert_eq!(
                alpha_at(tab, width - 2 * radius - 1, 0),
                255,
                "scale {scale}: the top edge is solid up to the right round"
            );
            assert_eq!(
                alpha_at(tab, width - radius - 1, 0),
                0,
                "scale {scale}: the tab's top-right corner is cut away too"
            );

            // Square-cut at the bottom: the tab body's own span is solid on the
            // last row, with no rounding and no hairline of its own.
            for x in [radius, width / 2, width - radius - 1] {
                assert_eq!(
                    alpha_at(tab, x, last_row),
                    255,
                    "scale {scale}: the bottom edge must be unbroken at x={x} — \
                     the tab and the content plane are one surface"
                );
            }
            // Red gate: a plain rounded rectangle ends at the body's own sides,
            // so these two pixels — one on each skirt — would be empty.
            assert_eq!(
                alpha_at(tab, radius - 1, last_row),
                255,
                "scale {scale}: the left skirt must carry the fill past the tab's own side"
            );
            assert_eq!(
                alpha_at(tab, width - radius, last_row),
                255,
                "scale {scale}: the right skirt must carry the fill past the tab's own side"
            );
            // And the flare thins out towards the silhouette's outer corner
            // rather than stopping short of it: one pixel in from either end of
            // the bottom row still carries ink, and it is *partial* ink, which
            // is the curve tapering to the corner point.
            for (side, x) in [("left", 1), ("right", width - 2)] {
                let alpha = alpha_at(tab, x, last_row);
                assert!(
                    alpha > 0 && alpha < 255,
                    "scale {scale}: the {side} skirt must taper to the outer corner, saw {alpha}"
                );
            }

            // Concave, not convex: the skirt's gradient circle is centred at the
            // silhouette's outer bottom corner, so the pixel at that centre is
            // empty on both sides. A convex corner would fill it.
            assert_eq!(
                alpha_at(tab, 0, height - radius),
                0,
                "scale {scale}: the left skirt must curve away from its own corner"
            );
            assert_eq!(
                alpha_at(tab, width - 1, height - radius),
                0,
                "scale {scale}: the right skirt must curve away from its own corner"
            );
        }
    }

    /// The corners are antialiased rather than stepped: along the top-left
    /// round there are partial-coverage pixels, which is the whole difference
    /// between an analytic curve and the nested quads it replaces.
    #[test]
    fn the_tab_corners_carry_partial_coverage_instead_of_a_staircase() {
        let radius = 14_u32;
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[ChromeSprite::new(
            ChromeMark::ActiveTab { radius_px: radius },
            [0.0, 0.0, 240.0, 68.0],
            [0x1b, 0x1b, 0x1b],
        )]);
        let tab = &icons[0];
        let partial = (0..radius * 2)
            .flat_map(|y| (0..radius * 2).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let alpha = alpha_at(tab, *x, *y);
                alpha > 0 && alpha < 255
            })
            .count();
        assert!(
            partial >= radius as usize,
            "an antialiased quarter-circle spends at least one partial pixel per row, saw {partial}"
        );
    }

    /// PIN (`+` hover fidelity) — a control's hover fill is **round**, and the
    /// round is analytic: its corners spend partial-coverage pixels, which is
    /// the whole difference between a curve and the staircase a stack of nested
    /// rectangles leaves.
    ///
    /// Red gate: this is the shape the `+` did not have. Drawn as a
    /// `ChromeQuad` its corner pixel was fully opaque — assertion one — and
    /// there were no partial pixels anywhere in it — assertion three.
    #[test]
    fn a_control_pill_is_round_at_its_corners_and_solid_in_its_middle() {
        for (side, radius) in [(28_u32, 6_u32), (42, 9), (56, 12), (17, 4), (34, 8)] {
            let mut rasters = ChromeMarkRasters::default();
            let icons = rasters.resolve(&[sprite(
                ChromeMark::ControlPill { radius_px: radius },
                side as f32,
                side as f32,
                [0x33, 0x44, 0x55],
            )]);
            let pill = icons.first().expect("a pill must rasterize");
            assert_eq!((pill.width_px, pill.height_px), (side, side));

            // Round: the box's own corner pixel is outside the shape, and a
            // radius in from it the fill is solid.
            for (x, y) in [(0, 0), (side - 1, 0), (0, side - 1), (side - 1, side - 1)] {
                assert_eq!(
                    alpha_at(pill, x, y),
                    0,
                    "radius {radius}: the pill's corner pixel ({x},{y}) must be cut away"
                );
            }
            assert_eq!(alpha_at(pill, side / 2, side / 2), 255);
            assert_eq!(
                alpha_at(pill, side / 2, 0),
                255,
                "radius {radius}: the top edge between the corners is solid"
            );
            assert_eq!(alpha_at(pill, 0, side / 2), 255);

            // Analytic, not stepped: a quarter circle spends at least one
            // partial pixel per row it crosses.
            let partial = (0..radius)
                .flat_map(|y| (0..radius).map(move |x| (x, y)))
                .filter(|(x, y)| {
                    let alpha = alpha_at(pill, *x, *y);
                    alpha > 0 && alpha < 255
                })
                .count();
            assert!(
                partial >= radius as usize,
                "radius {radius}: a rounded corner is not a staircase, saw {partial} partial pixels"
            );
        }
    }

    /// PIN — a pill is **opaque**, and the two radii the chrome uses are two
    /// rasters rather than one stretched.
    ///
    /// Opacity is the claim that matters. A translucent fill would have to be
    /// blended by the pipeline, and the pipeline blends in *linear* light while
    /// the design's renderer blends in sRGB: handing it `--active`'s own .09
    /// over the dark tab lands at 89 where the mock-up puts 48. Every chrome
    /// fill is pre-composited for exactly that reason, and this one is no
    /// exception.
    #[test]
    fn a_pill_is_opaque_and_each_radius_is_its_own_raster() {
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(
                ChromeMark::ControlPill { radius_px: 4 },
                17.0,
                17.0,
                [0xed, 0xed, 0xec],
            ),
            sprite(
                ChromeMark::ControlPill { radius_px: 6 },
                17.0,
                17.0,
                [0xed, 0xed, 0xec],
            ),
        ]);
        for icon in &icons {
            assert_eq!(
                alpha_at(icon, 8, 8),
                255,
                "a chrome fill carries no alpha of its own"
            );
            assert_eq!(rgb_at(icon, 8, 8), [0xed, 0xed, 0xec]);
        }
        assert_ne!(icons[0].key, icons[1].key, "the radius is part of identity");
    }

    /// PIN — the chevron is one arrow that turns over. The open glyph is the
    /// resting one rotated 180° about its own centre, so the two are mirror
    /// images across the symbol's mid-line rather than two drawings.
    #[test]
    fn the_chevron_turns_over_instead_of_swapping_glyphs() {
        let ink = [0x9d, 0x9d, 0x9d];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::Chevron { open: false }, 18.0, 12.0, ink),
            sprite(ChromeMark::Chevron { open: true }, 18.0, 12.0, ink),
        ]);
        let (down, up) = (&icons[0], &icons[1]);
        assert_ne!(down.key, up.key);
        let ink_of = |icon: &ChromeIcon, y: u32| {
            (0..icon.width_px)
                .map(|x| u32::from(alpha_at(icon, x, y)))
                .sum::<u32>()
        };
        // A down chevron carries its two arms at the top and its point at the
        // bottom; rotating it swaps which row is which.
        assert!(
            ink_of(down, 1) > ink_of(down, down.height_px - 2),
            "a resting chevron points down"
        );
        assert!(
            ink_of(up, up.height_px - 2) > ink_of(up, 1),
            "an open chevron points up"
        );
        for y in 0..down.height_px {
            let mirrored = down.height_px - 1 - y;
            let (a, b) = (ink_of(down, y), ink_of(up, mirrored));
            assert!(
                a.abs_diff(b) * 20 <= a.max(b).max(1) * 3,
                "row {y} of the turned arrow is not row {mirrored} of the resting one \
                 ({a} against {b}) — it is a second glyph, not the same one rotated"
            );
        }
    }

    /// A raster is produced once and reused while it stays on screen, and the
    /// map is trimmed to what the current frame actually asked for.
    #[test]
    fn rasters_are_reused_across_frames_and_retired_when_unused() {
        let mut rasters = ChromeMarkRasters::default();
        let first = rasters.resolve(&[sprite(ChromeMark::Folder, 13.0, 13.0, [1, 2, 3])]);
        let again = rasters.resolve(&[sprite(ChromeMark::Folder, 13.0, 13.0, [1, 2, 3])]);
        assert_eq!(first[0].key, again[0].key);
        assert!(
            Arc::ptr_eq(&first[0].rgba, &again[0].rgba),
            "an unchanged mark must not be rasterized twice"
        );
        assert_eq!(rasters.rasters.len(), 1);

        let recoloured = rasters.resolve(&[sprite(ChromeMark::Folder, 13.0, 13.0, [9, 9, 9])]);
        assert_ne!(
            first[0].key, recoloured[0].key,
            "colour is part of identity"
        );
        assert_eq!(
            rasters.rasters.len(),
            1,
            "the retired colour must not stay resident"
        );
    }

    /// A box with no pixels in it is not drawn — and asking for one does not
    /// poison the frame's other marks.
    #[test]
    fn a_collapsed_box_produces_no_mark() {
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::Folder, 0.0, 13.0, [1, 2, 3]),
            sprite(ChromeMark::Folder, 13.0, 13.0, [1, 2, 3]),
        ]);
        assert_eq!(icons.len(), 1);
    }
}
