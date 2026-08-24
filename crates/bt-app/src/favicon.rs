//! **The site's own icon, where a site has one** (`docs/DESIGN.md` §7.7 ②,
//! landed in §7.13).
//!
//! §7.7 ② asks for "the favicon where a site has one and this globe where it has
//! not", and until this module the second half was the whole answer — every
//! surface that draws a page drew [`crate::marks::ChromeMark::Globe`], so a
//! switcher full of pages could only be told apart by reading. What stood in the
//! way was named in `marks.rs` and in §7.11 ⑪: every mark in this window is a
//! vector symbol struck from a path list and cached by name, and a favicon is a
//! raster the engine fetches at runtime.
//!
//! # This is not a second pipeline
//!
//! It looked like one and it is not. `bt_render::ChromeIcon` — what
//! [`crate::marks::ChromeMarkRasters`] hands the renderer — has been *pixels
//! plus a key* since it existed, and W2 slice ⑥ already sends a page's last
//! frame down it without rasterizing anything (`web_thumb`, §7.11 ⑤). So the
//! favicon joins the mark channel at the same seam the thumbnail joins the
//! picture channel: this module owns the bytes and the identity, `marks`
//! substitutes them for the globe's raster at the moment it would have struck
//! one, and **no drawing site learns a new word**.
//!
//! That is why [`FaviconId`] exists and why it is a bare number. A mark travels
//! through this window as a `Copy` value in maps, tuples and comparisons —
//! `leaf_marks`, `pane_mark`, `preview_row_mark`, `tab_mark`, the drag ghost,
//! the collapsed bar, the focus card, the download sheet. Hanging a `String`
//! site on it would have made `ChromeMark` non-`Copy` and rewritten every one of
//! those; a number minted here keeps the mark exactly what it was and still says
//! which pixels it means.
//!
//! # What it is keyed by, and why that is a site rather than a seat
//!
//! A favicon belongs to a **site** (`webnav::site_key` — `scheme://host[:port]`),
//! not to a pane. Three consequences, all of them wanted:
//!
//! * A page that navigates within one server keeps its icon without re-asking.
//! * Two panes on one server share one texture rather than minting two.
//! * A **row** can have one. The switcher, the Recent vault and the restore
//!   prompt draw pages nothing has opened — they have a URL and no seat — and a
//!   store keyed by seat could never have answered them. That is the whole of
//!   the 2026-08-23 ruling this slice comes from: "切换器/Recent 里网页行全是
//!   地球标、只能靠读文字区分".
//!
//! And one that has to be said out loud rather than discovered: a seat that goes
//! away does **not** take the entry with it. It does not need to — a seat with no
//! page has no URL to look one up with, so it wears the globe by construction —
//! and taking it would blank the switcher row for a site the session has plainly
//! visited. The store's life is the session's; §7.7 ② is about what a *row*
//! wears, and a row outlives the pane it was opened in.
//!
//! # Nothing here reaches the disk
//!
//! Deliberately, and the reason is the one §3 already gave about URLs: this
//! window persists *places*, in the clear, and a favicon cache on disk would be
//! a second record of where somebody has been — one with no schema, no version
//! and no reader that could be asked to forget. So a restored session wears
//! globes until its pages come up and say otherwise, which is the honest picture
//! of what the window knows at that moment.

use std::collections::HashMap;
use std::sync::Arc;

/// **Which stored icon a mark means.** Minted by [`Favicons`] and by nothing
/// else.
///
/// A serial rather than a hash of the site, because the identity it has to carry
/// is *these pixels* and not *this server*: a site that swaps its icon while the
/// window is open must mint a new one, or the GPU texture cache — which is asked
/// by key and answers with whatever it kept — would go on drawing the old icon
/// for as long as the window lived. `web_thumb`'s `web-thumb:<tab>:<seat>:<serial>`
/// is the same device for the same reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaviconId(u64);

impl FaviconId {
    /// A named id for a test that is about **what a mark carries** rather than
    /// about what the store holds.
    ///
    /// The strip's own gate is one of these: it asks whether the row draws what
    /// the head draws, and standing up an engine, a page and a decode to ask it
    /// would make a test of one sentence depend on five.
    #[cfg(test)]
    #[must_use]
    pub fn for_tests(serial: u64) -> Self {
        Self(serial)
    }

    /// The texture key these pixels are drawn under at one physical size.
    ///
    /// `favicon:` and not `chrome-mark:`: the two share one texture cache, and a
    /// key that began the same way would be a build in which a site could evict
    /// a glyph.
    #[must_use]
    pub fn texture_key(self, width_px: u32, height_px: u32) -> String {
        format!("favicon:{}:{width_px}x{height_px}", self.0)
    }
}

/// How many sites the session keeps icons for.
///
/// A cap and not an unbounded map, because the map's key is "a server somebody
/// visited" and a window can be open for weeks. The number is chosen against
/// what an entry costs: a source capped at [`SOURCE_CEILING_PX`] square is 16 KB
/// of RGBA, so the whole store is bounded at roughly a megabyte and a half
/// including the resamples — and 64 distinct servers in one session is already
/// well past what the terminal's own browser is for.
///
/// Eviction is by **least recently learned**, which is the only recency this
/// store honestly has: a lookup is `&self` (the drawing path asks it on every
/// chrome rebuild) and a counter bumped from there would be a cache mutating
/// itself while a frame reads it.
pub const CAPACITY: usize = 64;

/// The largest square a stored source is kept at.
///
/// The icon is only ever drawn in a mark's box — 14 logical pixels (§7.7 ②:
/// "两者共用 14px 的同一个盒子") — so at a 300% display that is 42 physical
/// pixels and this is comfortably above it. Sites do serve 192- and 512-pixel
/// icons; keeping one of those whole would be storing a quarter of a megabyte to
/// draw fourteen pixels with.
pub const SOURCE_CEILING_PX: u32 = 64;

/// One decoded icon, ready to be handed to the renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
}

/// One site's icon: the decoded source, and the boxes it has been asked for.
#[derive(Debug)]
struct Site {
    id: FaviconId,
    source: Raster,
    /// Resamples, by the physical box they were cut to.
    ///
    /// Kept because the drawing path asks for the same one or two boxes on every
    /// chrome rebuild and a resample per frame per row would be this slice's
    /// whole cost; small because the boxes a window has are its scale, and a
    /// window has one.
    sized: HashMap<(u32, u32), Raster>,
    /// Which learn this was, for the eviction order.
    learned: u64,
}

/// **Every icon this session has been told about**, by site.
#[derive(Debug, Default)]
pub struct Favicons {
    sites: HashMap<String, Site>,
    by_id: HashMap<FaviconId, String>,
    next_serial: u64,
    learns: u64,
}

impl Favicons {
    /// **File a site's icon**, replacing whatever it had.
    ///
    /// `false` for bytes that will not decode — the engine handed over something
    /// this build cannot read, which is a fact about the answer and not a reason
    /// to invent pixels or to ask again. The site keeps the icon it already had
    /// rather than losing one to a bad re-announcement, because a decode that
    /// failed says nothing about whether the last one was right.
    ///
    /// `true` means the chrome has to be rebuilt: a new id is live and the marks
    /// that name this site now mean different pixels.
    pub fn learn(&mut self, site: &str, png: &[u8]) -> bool {
        let Some(source) = decode(png) else {
            return false;
        };
        if self
            .sites
            .get(site)
            .is_some_and(|held| held.source == source)
        {
            // The same icon announced twice — a reload, or a second page on the
            // same server. Re-minting here would retire a texture the renderer
            // is holding and upload the identical pixels under a new name.
            return false;
        }
        self.evict_until_there_is_room_for_one_more();
        self.next_serial += 1;
        self.learns += 1;
        let id = FaviconId(self.next_serial);
        if let Some(previous) = self.sites.insert(
            site.to_owned(),
            Site {
                id,
                source,
                sized: HashMap::new(),
                learned: self.learns,
            },
        ) {
            self.by_id.remove(&previous.id);
        }
        self.by_id.insert(id, site.to_owned());
        true
    }

    /// **This site has no icon** — the page said so, or it never had one.
    ///
    /// The silent half of the failure vocabulary (§7.7 ②'s "没有画本类的地球"):
    /// what a caller does with this is nothing at all, and what the next chrome
    /// rebuild does is draw the globe, because [`Self::of_url`] now answers
    /// `None`.
    pub fn forget(&mut self, site: &str) -> bool {
        match self.sites.remove(site) {
            Some(gone) => {
                self.by_id.remove(&gone.id);
                true
            }
            None => false,
        }
    }

    /// **Which icon a URL wears**, or `None` for a site with none.
    ///
    /// `&self` on purpose: this is the question every drawing site asks while it
    /// builds a frame, and a store that rearranged itself to answer it would be
    /// mutating a cache in the middle of the read that walks it.
    #[must_use]
    pub fn of_url(&self, url: &str) -> Option<FaviconId> {
        self.id_for(&crate::webnav::site_key(url)?)
    }

    /// The same by site key, for a caller that already has one.
    #[must_use]
    pub fn id_for(&self, site: &str) -> Option<FaviconId> {
        self.sites.get(site).map(|held| held.id)
    }

    /// **The pixels, cut to the box a mark is drawn in.**
    ///
    /// `None` for an id the store no longer holds, and that is the whole of what
    /// the drawing path needs to hear: a mark whose site has been forgotten or
    /// evicted between the frame that chose it and the frame that draws it falls
    /// through to the vector globe it was always going to fall through to.
    pub fn raster(&mut self, id: FaviconId, width_px: u32, height_px: u32) -> Option<Raster> {
        if width_px == 0 || height_px == 0 {
            return None;
        }
        let site = self.by_id.get(&id)?;
        let held = self.sites.get_mut(site)?;
        if let Some(kept) = held.sized.get(&(width_px, height_px)) {
            return Some(kept.clone());
        }
        let cut = resample(&held.source, width_px, height_px);
        held.sized.insert((width_px, height_px), cut.clone());
        Some(cut)
    }

    /// How many sites are held, for the test that holds the cap.
    ///
    /// Test-only rather than public: the count is not a fact any surface draws,
    /// and a store that offered one would invite somebody to draw it.
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.sites.len()
    }

    fn evict_until_there_is_room_for_one_more(&mut self) {
        while self.sites.len() >= CAPACITY {
            let Some(oldest) = self
                .sites
                .iter()
                .min_by_key(|(_, held)| held.learned)
                .map(|(site, _)| site.clone())
            else {
                return;
            };
            self.forget(&oldest);
        }
    }
}

/// **Decode what the engine handed over**, and hold it to a sane size.
///
/// PNG by name rather than by sniffing, because the format is not a guess: this
/// window asks `GetFavicon` for `COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG` and the
/// engine re-encodes whatever the site actually served — which is very often an
/// `.ico`, a format nothing in this workspace can read. Asking for PNG is how
/// the `.ico` problem is not this module's problem.
///
/// The ceiling is applied here rather than at the draw, so that the megabyte a
/// site with a 512-pixel icon would otherwise cost is never stored at all.
#[must_use]
pub fn decode(png: &[u8]) -> Option<Raster> {
    let decoded = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    if width == 0 || height == 0 {
        return None;
    }
    let source = Raster {
        rgba: Arc::from(rgba.into_raw()),
        width_px: width,
        height_px: height,
    };
    if width <= SOURCE_CEILING_PX && height <= SOURCE_CEILING_PX {
        return Some(source);
    }
    let scale = f64::from(SOURCE_CEILING_PX) / f64::from(width.max(height));
    let ceiling = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
    Some(resample(&source, ceiling(width), ceiling(height)))
}

/// Cut a decoded icon to one box.
///
/// **`Lanczos3` and not the `Triangle` `web_thumb::shrink` picks**, and the two
/// choices are the same argument reaching opposite answers. A page thumbnail is
/// a 1146-pixel photograph shrunk past a factor of four into a card nobody reads
/// detail off; there the sharper filter buys nothing visible and costs three
/// times as much. A favicon is a 32-pixel drawing landing in a 14-pixel box
/// beside a word — it is *all* detail, it is looked at to tell one row from
/// another, and the whole cost of the sharper filter at this size is a few
/// microseconds paid once per site per box.
#[must_use]
fn resample(source: &Raster, width_px: u32, height_px: u32) -> Raster {
    let buffer = image::RgbaImage::from_raw(
        source.width_px,
        source.height_px,
        source.rgba.as_ref().to_vec(),
    )
    .expect("a stored favicon's buffer is its own width times its own height");
    let cut = image::imageops::resize(
        &buffer,
        width_px,
        height_px,
        image::imageops::FilterType::Lanczos3,
    );
    Raster {
        rgba: Arc::from(cut.into_raw()),
        width_px,
        height_px,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real PNG of one flat colour, so that a resample's own answer is
    /// checkable: every pixel of a solid square stays that colour whatever the
    /// filter and whatever the box.
    fn a_png(width: u32, height: u32, colour: [u8; 4]) -> Vec<u8> {
        let mut buffer = image::RgbaImage::new(width, height);
        for pixel in buffer.pixels_mut() {
            *pixel = image::Rgba(colour);
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("an in-memory PNG encodes");
        bytes.into_inner()
    }

    /// **Red gate: a site that has an icon stops wearing the globe, and the
    /// pixels it wears are its own.**
    ///
    /// The whole slice in one claim — a URL goes in, an id comes out, and the id
    /// resolves to the bytes the engine handed over rather than to anything this
    /// window drew.
    ///
    /// MUTATION: make `learn` return `false` without storing, or make `of_url`
    /// answer `None`, and this fails at the first assertion — which is the state
    /// of the tree before this slice.
    #[test]
    fn a_site_with_an_icon_is_not_a_globe() {
        let mut store = Favicons::default();
        assert!(store.learn("http://localhost:8642", &a_png(32, 32, [10, 20, 30, 255])));
        let id = store
            .of_url("http://localhost:8642/docs?q=1#top")
            .expect("a page on a site that has an icon wears it");
        let raster = store.raster(id, 14, 14).expect("the icon cuts to the box");
        assert_eq!((raster.width_px, raster.height_px), (14, 14));
        assert_eq!(raster.rgba.len(), 14 * 14 * 4);
        assert_eq!(
            &raster.rgba[..4],
            &[10, 20, 30, 255],
            "a solid icon resamples to the same solid colour"
        );
    }

    /// **Red gate: a site with no icon is a globe, and asking cost nothing.**
    ///
    /// §7.7 ②'s second half and the slice's fourth rule at once: the failure
    /// vocabulary is "there is no id", there is no error to report and there is
    /// nothing to retry.
    ///
    /// MUTATION: have `of_url` answer `Some` for an unknown site and this fails.
    #[test]
    fn a_site_with_no_icon_has_no_id_at_all() {
        let mut store = Favicons::default();
        assert_eq!(store.of_url("http://localhost:8642/"), None);
        store.learn("http://localhost:8642", &a_png(32, 32, [1, 2, 3, 255]));
        assert!(store.of_url("http://localhost:8642/").is_some());
        assert!(store.forget("http://localhost:8642"));
        assert_eq!(
            store.of_url("http://localhost:8642/"),
            None,
            "a page that said it has no icon goes back to the globe"
        );
    }

    /// **Red gate: bytes that will not decode leave the site as it was.**
    ///
    /// The other half of rule four. A refusal is not a reason to drop an icon
    /// that was already right, and it is not a reason to say anything.
    ///
    /// MUTATION: make `learn` `forget` the site before decoding and the second
    /// assertion fails.
    #[test]
    fn an_icon_that_will_not_decode_changes_nothing() {
        let mut store = Favicons::default();
        store.learn("https://example.test", &a_png(16, 16, [9, 9, 9, 255]));
        let before = store.of_url("https://example.test/").expect("filed");
        assert!(!store.learn("https://example.test", b"not a png at all"));
        assert_eq!(
            store.of_url("https://example.test/"),
            Some(before),
            "a decode that failed says nothing about the icon already held"
        );
    }

    /// **Red gate: a site that swaps its icon mints a new id.**
    ///
    /// The texture cache is asked by key and answers with whatever it kept, so
    /// an id that stood still would pin the old pixels for the life of the
    /// window.
    ///
    /// MUTATION: reuse the previous id in `learn` and this fails on the
    /// inequality — while the pixels assertion still passes, which is exactly
    /// the shape of the bug this pins.
    #[test]
    fn a_site_that_changes_its_icon_changes_its_identity() {
        let mut store = Favicons::default();
        store.learn("https://example.test", &a_png(32, 32, [200, 0, 0, 255]));
        let first = store.of_url("https://example.test/").expect("filed");
        assert!(store.learn("https://example.test", &a_png(32, 32, [0, 200, 0, 255])));
        let second = store.of_url("https://example.test/").expect("re-filed");
        assert_ne!(first, second, "different pixels are a different identity");
        assert_ne!(
            first.texture_key(14, 14),
            second.texture_key(14, 14),
            "and therefore a different texture key"
        );
        assert_eq!(store.raster(first, 14, 14), None, "the old id is retired");
        assert_eq!(
            &store.raster(second, 14, 14).expect("the new id draws").rgba[..4],
            &[0, 200, 0, 255]
        );
    }

    /// **The same icon announced twice is not a change.**
    ///
    /// A reload, or a second pane on the same server, re-announces the icon the
    /// store already has; minting for that would retire a live texture and
    /// upload the identical pixels under a new name once per navigation.
    #[test]
    fn the_same_icon_twice_is_not_a_new_identity() {
        let mut store = Favicons::default();
        let png = a_png(32, 32, [7, 7, 7, 255]);
        assert!(store.learn("https://example.test", &png));
        let first = store.of_url("https://example.test/").expect("filed");
        assert!(
            !store.learn("https://example.test", &png),
            "the same pixels are not a rebuild"
        );
        assert_eq!(store.of_url("https://example.test/"), Some(first));
    }

    /// **Red gate: scheme, host and port are the identity, and the path is
    /// not.**
    ///
    /// One server's pages share one icon, and two servers never share one. The
    /// second half is the security-shaped one: `http` and `https` on one host
    /// are two servers.
    ///
    /// MUTATION: key on `webnav::site_label` (host and port, no scheme) and the
    /// last assertion fails — the plain-text server wears the secure one's icon.
    #[test]
    fn one_server_is_one_icon_and_two_servers_are_two() {
        let mut store = Favicons::default();
        store.learn("https://example.test", &a_png(16, 16, [1, 1, 1, 255]));
        let id = store.of_url("https://example.test/").expect("filed");
        assert_eq!(store.of_url("https://example.test/a/b?c=d#e"), Some(id));
        assert_eq!(
            store.of_url("https://example.test:443/"),
            Some(id),
            "the default port is the same site wearing a hat"
        );
        assert_eq!(store.of_url("https://other.test/"), None);
        assert_eq!(
            store.of_url("http://example.test/"),
            None,
            "plain text and TLS on one host are two servers"
        );
    }

    /// **Red gate: a huge icon is held down to the ceiling before it is
    /// stored.**
    ///
    /// A site may serve a 512-pixel icon; this window draws fourteen of them.
    ///
    /// MUTATION: return the decoded source unchanged from `decode` and the
    /// stored side is 512.
    #[test]
    fn an_oversized_icon_is_cut_down_before_it_is_kept() {
        let raster = decode(&a_png(512, 256, [4, 5, 6, 255])).expect("decodes");
        assert_eq!(
            (raster.width_px, raster.height_px),
            (SOURCE_CEILING_PX, SOURCE_CEILING_PX / 2),
            "the long side lands on the ceiling and the aspect is kept"
        );
        assert_eq!(
            raster.rgba.len() as u32,
            raster.width_px * raster.height_px * 4
        );
    }

    /// A small icon is kept exactly as it arrived — the ceiling is a ceiling and
    /// not a target, and upscaling a 16-pixel drawing into 64 pixels of storage
    /// would be inventing detail and paying to keep it.
    #[test]
    fn a_small_icon_is_kept_at_its_own_size() {
        let raster = decode(&a_png(16, 16, [4, 5, 6, 255])).expect("decodes");
        assert_eq!((raster.width_px, raster.height_px), (16, 16));
    }

    /// **Red gate: the box is cut once per box, not once per frame.**
    ///
    /// The drawing path asks on every chrome rebuild; a resample there would be
    /// this slice's whole cost.
    ///
    /// MUTATION: drop the `sized` insert and the two rasters are still equal —
    /// so this asserts on the pointer, which is the only observable difference
    /// between a cache and a recomputation.
    #[test]
    fn a_box_is_cut_once_and_then_handed_out() {
        let mut store = Favicons::default();
        store.learn("https://example.test", &a_png(32, 32, [8, 8, 8, 255]));
        let id = store.of_url("https://example.test/").expect("filed");
        let first = store.raster(id, 14, 14).expect("cut");
        let second = store.raster(id, 14, 14).expect("cut again");
        assert!(
            Arc::ptr_eq(&first.rgba, &second.rgba),
            "the second ask is the first ask's pixels"
        );
        let bigger = store.raster(id, 28, 28).expect("a second box");
        assert_eq!((bigger.width_px, bigger.height_px), (28, 28));
        assert!(
            !Arc::ptr_eq(&first.rgba, &bigger.rgba),
            "a different box is different pixels"
        );
    }

    /// **Red gate: the store is bounded.**
    ///
    /// Its key is "a server somebody visited" and a window can be open for
    /// weeks.
    ///
    /// MUTATION: remove the eviction call and the length walks past the cap.
    #[test]
    fn the_store_does_not_grow_without_end() {
        let mut store = Favicons::default();
        for site in 0..CAPACITY * 2 {
            let colour = [(site % 251) as u8, 1, 2, 255];
            assert!(store.learn(&format!("https://s{site}.test"), &a_png(8, 8, colour)));
        }
        assert_eq!(store.len(), CAPACITY);
        assert_eq!(
            store.of_url("https://s0.test/"),
            None,
            "the first server learned is the first one dropped"
        );
        assert!(
            store
                .of_url(&format!("https://s{}.test/", CAPACITY * 2 - 1))
                .is_some(),
            "the last server learned is still held"
        );
    }

    /// A box with no area is refused before the resampler is asked — a zero-wide
    /// mark is a layout that has not settled, not a request for zero pixels.
    #[test]
    fn a_box_with_no_area_is_refused() {
        let mut store = Favicons::default();
        store.learn("https://example.test", &a_png(8, 8, [1, 2, 3, 255]));
        let id = store.of_url("https://example.test/").expect("filed");
        assert_eq!(store.raster(id, 0, 14), None);
        assert_eq!(store.raster(id, 14, 0), None);
    }

    /// **A texture key never begins the way a mark's does.**
    ///
    /// The two share one GPU cache; a key that began `chrome-mark:` would be a
    /// build in which a site could evict a glyph — or, worse, be handed one.
    #[test]
    fn a_favicon_key_is_not_a_mark_key() {
        let key = FaviconId(7).texture_key(14, 14);
        assert_eq!(key, "favicon:7:14x14");
        assert!(!key.starts_with("chrome-mark:"));
        assert_ne!(
            FaviconId(7).texture_key(14, 14),
            FaviconId(8).texture_key(14, 14)
        );
        assert_ne!(
            FaviconId(7).texture_key(14, 14),
            FaviconId(7).texture_key(28, 28)
        );
    }
}
