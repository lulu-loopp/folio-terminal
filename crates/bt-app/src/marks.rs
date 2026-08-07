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

use bt_render::ChromeIcon;

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
            Self::ProfilePowerShell => "p-pwsh",
            Self::File => "i-file",
            Self::Folder => "i-folder",
            Self::Panel => "i-panel",
            Self::ActiveTab { .. } => "tab",
            Self::TabBody { .. } => "tab-body",
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
            });
            kept.insert(key, raster);
        }
        self.rasters = kept;
        icons
    }
}

fn mark_key(sprite: &ChromeSprite, width_px: u32, height_px: u32) -> String {
    let [r, g, b] = sprite.color;
    let mut key = format!("chrome-mark:{}", sprite.mark.id());
    if let ChromeMark::ActiveTab { radius_px } | ChromeMark::TabBody { radius_px } = sprite.mark {
        let _ = write!(key, ":r{radius_px}");
    }
    let _ = write!(key, ":{width_px}x{height_px}");
    if sprite.mark.takes_current_color() {
        let _ = write!(key, ":{r:02x}{g:02x}{b:02x}");
    }
    key
}

fn rasterize(sprite: &ChromeSprite, width_px: u32, height_px: u32) -> Option<Raster> {
    let document = svg_document(sprite, width_px, height_px)?;
    let raster = bt_math::rasterize_svg_document(document.as_bytes()).ok()?;
    Some(Raster {
        rgba: Arc::from(raster.rgba),
        width_px: raster.width_px,
        height_px: raster.height_px,
    })
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
        // Handled before this function is reached; its geometry is generated,
        // not quoted.
        ChromeMark::ActiveTab { .. } => 8,
        ChromeMark::TabBody { .. } => 8,
    }
}

const SYMBOL_VIEW_BOX: [&str; 9] = [
    "0 0 24 24",
    "0 0 10 10",
    "0 0 10 10",
    "0 0 10 10",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
];

/// The `<symbol>` bodies, byte for byte from `design/ui-mockup.html` (the
/// `<svg style="display:none">` block near the top of `<body>`).
const SYMBOL_BODY: [&str; 9] = [
    // #i-gear
    r#"<path fill="currentColor" d="M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.488.488 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.484.484 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z"/>"#,
    // #i-min
    r#"<path d="M0 5h10" fill="none" stroke="currentColor" stroke-width="1"/>"#,
    // #i-max
    r#"<rect x="0.5" y="0.5" width="9" height="9" rx="1.8" fill="none" stroke="currentColor" stroke-width="1"/>"#,
    // #i-close
    r#"<path d="M0.5 0.5l9 9M9.5 0.5l-9 9" fill="none" stroke="currentColor" stroke-width="1"/>"#,
    // #i-plus
    r#"<path d="M8 3v10M3 8h10" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/>"#,
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
        ChromeSprite {
            mark,
            rect: [0.0, 0.0, width, height],
            color,
        }
    }

    fn alpha_at(icon: &ChromeIcon, x: u32, y: u32) -> u8 {
        icon.rgba[((y * icon.width_px + x) * 4 + 3) as usize]
    }

    fn rgb_at(icon: &ChromeIcon, x: u32, y: u32) -> [u8; 3] {
        let base = ((y * icon.width_px + x) * 4) as usize;
        [icon.rgba[base], icon.rgba[base + 1], icon.rgba[base + 2]]
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
            let icons = rasters.resolve(&[ChromeSprite {
                mark: ChromeMark::ActiveTab { radius_px: radius },
                rect: [0.0, 0.0, width as f32, height as f32],
                color: [0x1b, 0x1b, 0x1b],
            }]);
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
        let icons = rasters.resolve(&[ChromeSprite {
            mark: ChromeMark::ActiveTab { radius_px: radius },
            rect: [0.0, 0.0, 240.0, 68.0],
            color: [0x1b, 0x1b, 0x1b],
        }]);
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
