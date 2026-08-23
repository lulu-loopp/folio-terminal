//! **A page's own last frame, kept for the card that draws it**
//! (`docs/DESIGN.md` §7.11, Web 预览块 W2 片⑥).
//!
//! # The two numbers this module exists to reconcile
//!
//! `focus_thumb` spends a budget of `FULL_BLAST_BUDGET_MS = 3.0` per frame at
//! 10 Hz, and the only pixel channel a hosted page offers —
//! `ICoreWebView2::CapturePreview` — was measured at **52.7 ms** by the pixel
//! matrix (`w0p-evidence` gate 2). Two orders of magnitude apart, and the whole
//! of this slice was deciding what to do about that.
//!
//! The reconciliation is that **the two numbers are not the same quantity**.
//! 52.7 ms is a *latency*, measured from the call to the completion handler, and
//! almost none of it is spent on the caller's thread. Gate 11 took the numbers
//! apart on this machine (release, page at 1146 × 777):
//!
//! | what | median |
//! |---|---|
//! | the synchronous ask (`CapturePreview` returning) | **0.115 ms** |
//! | the wait, on the engine's own clock | 84 ms |
//! | reading the encoded bytes back out of the stream | **0.014 ms** |
//! | decoding the PNG and resampling it to a card | 18 ms |
//!
//! So a window pays **0.13 ms** to take one picture, plus a decode that must not
//! happen on its thread. Asked and *waited for* inside a frame, the loop
//! collapses from 8.30 ms a frame to 84.31 ms with 180 frames out of 180 over
//! 20 ms; asked and collected on a later pump, the same loop measured **8.297 ms
//! against the baseline's 8.299 ms, with zero frames over 20 ms**. The lane below
//! is the second arrangement, and it is the only one that exists here.
//!
//! # The fact that shapes everything else
//!
//! **A hidden WebView never completes a capture.** Measured on 2026-08-20,
//! re-measured twice on 2026-08-22, and re-measured again at card scale by gate
//! 11: the completion handler is simply not called. And in a focus column every
//! card but one is a background tab, whose page is `SetIsVisible(false)` by
//! §7.8 ⑧.
//!
//! So a live thumbnail of a background page is not a thing that can be built —
//! `plan.md` §1 says so outright ("F2 活缩略图与低延迟远程逐帧,公开 API 无受
//! 支持路径") — and what *can* be built is the one thing the plan then names:
//! **the last frame that page was on the glass for, kept**. That is this module.
//! A web card shows the page as it last stood on screen; a page that has not
//! stood on screen since this window opened shows the face it has always shown.
//!
//! # Where the four gates end and this begins
//!
//! `focus_thumb`'s gates govern **asking**: the mode is off, nothing is asked;
//! the card is scrolled out of the column, nothing is asked. What they do *not*
//! govern is the picture itself, and the difference is forced by the medium. A
//! terminal's card is dropped when it scrolls out of the column and re-projected
//! fresh when it comes back, because the grid it is a picture of is in memory and
//! can be asked again at any moment. A page's pixels exist only while the page is
//! on the glass. Dropping them on a scroll would mean scrolling the column blanks
//! every web card **permanently** — nothing could ever refill them. So the
//! picture belongs to the *seat*, not to the card, and it dies when the seat does
//! or when the page under it becomes a different page.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use crate::LeafId;

/// **How often one seat's page may be re-photographed** — once every two
/// seconds.
///
/// Twenty times slower than `focus_thumb::MIN_INTERVAL`, and chosen against the
/// same thing that number was: what the picture is *of*. A card's terminal seat
/// is a stream of new rows and 10 Hz is the rate at which it stops being worth
/// redrawing; a page is a document that mostly sits still, and the one card that
/// can be photographed at all is the card of the tab whose page is on the stage
/// two hundred pixels to the right, already at full size. There is nothing on it
/// a reader can only learn from the thumbnail, so the thumbnail's job is to be
/// *right by the time you switch away from it*, which two seconds is.
///
/// It also bounds the standing cost to arithmetic anyone can check: 0.13 ms of
/// the asking thread every 2 s is **0.0065 ms per 10 Hz pass**, against a budget
/// of 3.0.
pub const CAPTURE_INTERVAL: Duration = Duration::from_secs(2);

/// The largest picture this lane will keep for one seat, in pixels.
///
/// A card body is 263 logical pixels wide and at most 320 tall, so at 200% that
/// is 526 × 640 — 1.3 MB of RGBA. The cap is stated as an area rather than as a
/// pair so that a very wide seat in a very short card is refused by the same
/// rule, and it is here rather than at the resample so that a demand asking for
/// something absurd is refused *before* a browser is asked for a picture.
const MAX_PICTURE_PIXELS: u32 = 526 * 640;

/// Everything about one web seat that the capture decision is made of.
///
/// Read off the seat in one go — see `webhost::WebSeat::capture_facts` — because
/// every field is that type's own state and a caller assembling them one at a
/// time would be four chances to ask a stale question.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeatFacts {
    /// The engine has actually been told this page is visible.
    ///
    /// **The gate the whole lane stands on**: a hidden WebView's
    /// `CapturePreview` does not come back, so asking one is not a wasted call,
    /// it is a slot held open for ever.
    pub on_glass: bool,
    /// `WebSeat::is_closing` — the controller is gone and the browser is being
    /// waited for. There is nothing to photograph and up to ten seconds of
    /// waiting in which to ask for it.
    pub closing: bool,
    /// A document has actually loaded, and no failure card stands in its place.
    pub committed: bool,
    /// A capture is already out and unanswered.
    pub capturing: bool,
    /// The viewport size the engine was last given, which is the size any
    /// picture of it will come back at.
    pub size: Option<(u32, u32)>,
}

/// One visible card's web pane, and the box its picture would be drawn in.
pub struct PageDemand {
    /// **A [`LeafId`] and not a seat number, since F1b′.** This lane's store is
    /// the window's, and a window's cards are every tab it holds: a seat number
    /// restarts at one in each of them, so two tabs' pages would share one slot
    /// and each would keep dropping the other's picture as stale.
    pub leaf: LeafId,
    /// The seat's identity — the last URL that actually committed. What the
    /// picture would be *of*, and therefore what makes an older one stale.
    pub url: String,
    pub facts: SeatFacts,
    /// The mini cell this seat occupies on its card, in physical pixels. The
    /// resample's target, so that what is uploaded is the size that is drawn
    /// rather than a pane's worth of pixels squeezed by the sampler.
    pub target: (u32, u32),
}

/// One page's last frame, at the size a card draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// The shared texture cache's identity for these pixels: the seat, and how
    /// many pictures it has had. A serial rather than a hash of the bytes,
    /// because two consecutive frames of a still page are byte-identical and
    /// keying on content would make "the picture was refreshed" and "the picture
    /// is the same one" indistinguishable to a cache that is asked to hold both.
    pub key: String,
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
}

/// What one seat's slot holds.
///
/// Public only as an **opaque** value, so that [`WebThumbs::take`] and
/// [`WebThumbs::put`] can carry one whole record between two windows without
/// anything outside this module being able to read or write a field of it — a
/// slot half-copied is exactly how an ask in flight lands on the wrong page.
#[derive(Debug)]
pub struct Entry {
    /// The URL the picture — or the ask in flight — is of.
    url: String,
    picture: Option<Picture>,
    /// How many pictures this seat has had. Part of [`Picture::key`], and the
    /// whole of the projection's damage key: a card re-projects when, and only
    /// when, this moves.
    serial: u64,
    /// The ask that is out, if there is one.
    asked: Option<Asked>,
    /// When the last ask was made. The clock [`CAPTURE_INTERVAL`] is measured
    /// on, and it starts at the *ask* rather than at the answer so that a slow
    /// engine cannot be asked twice as often as a fast one.
    started: Option<Instant>,
    /// The viewport the last picture was taken at, so that a pane which has
    /// changed shape is owed a fresh one at once.
    source: Option<(u32, u32)>,
    /// **The bytes the engine last handed over**, kept so that the next answer
    /// can be compared with them.
    ///
    /// This is gate 3 — `focus_thumb`'s damage gate — reaching the one kind of
    /// card that could not have it. A terminal seat is asked "has your screen
    /// moved" and answers with an integer; a page cannot be asked anything at
    /// all until it has been photographed, so the cheapest place the question
    /// can be put is *after* the photograph and *before* the 18 ms decode and
    /// the megabyte upload behind it. A still page therefore costs one capture
    /// every two seconds and nothing else — no decode, no resample, no texture,
    /// and no re-projection of its card.
    ///
    /// The encoded PNG and not the decoded pixels, because the encoded form is
    /// what arrives and comparing it is a 40 KB memcmp against 18 ms of work.
    /// It is only ever an optimisation: an encoder that answered differently for
    /// identical pixels would simply leave this never firing.
    encoded: Option<Vec<u8>>,
}

#[derive(Debug)]
struct Asked {
    ticket: u64,
    url: String,
    target: (u32, u32),
}

/// A picture that has arrived and has not been shrunk yet — the unit of work the
/// window hands to [`PageShrinker`].
#[derive(Debug)]
pub struct ShrinkJob {
    pub leaf: LeafId,
    pub ticket: u64,
    pub png: Vec<u8>,
    pub target: (u32, u32),
}

/// A finished one, on its way back.
#[derive(Debug)]
pub struct ShrunkPicture {
    pub leaf: LeafId,
    pub ticket: u64,
    /// `None` when the bytes would not decode. The slot is released either way;
    /// the pane keeps whatever picture it already had.
    pub rgba: Option<(Vec<u8>, u32, u32)>,
}

/// What this window's capture lane did, in counts.
///
/// Counters and not timings, for `focus_thumb::ThumbStats`' reason: the claims
/// being made are "nothing was asked of a hidden page" and "an idle column asks
/// nothing", and a counter that stayed put is that claim exactly. Each `skipped_`
/// field names **which** refusal, so a regression that moved work from one to
/// another cannot hide inside a total.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WebThumbStats {
    /// Captures actually asked for.
    pub captures: u64,
    /// Pictures that landed and were stored.
    pub pictures: u64,
    /// Seats not asked because their page is not on the glass — a background
    /// tab, a modal, a pane the fit ladder squeezed out. Every card but the
    /// active tab's, most of the time.
    pub skipped_hidden: u64,
    /// Seats not asked because they are closing.
    pub skipped_closing: u64,
    /// Seats not asked because nothing has committed on them yet, or a failure
    /// card stands where the page would be.
    pub skipped_blank: u64,
    /// Seats not asked because their last ask is still out.
    pub skipped_in_flight: u64,
    /// Seats not asked because the last one was less than [`CAPTURE_INTERVAL`]
    /// ago.
    pub skipped_throttled: u64,
    /// Answers that were byte-for-byte the last one — a page that has not
    /// changed. The decode, the resample and the upload behind it are all
    /// refused.
    pub skipped_unchanged: u64,
    /// Answers thrown away because the seat had moved on — it navigated, or it
    /// was invalidated, between the ask and the answer.
    pub dropped_stale: u64,
}

/// Every web pane's last frame, and the clock that refreshes them.
///
/// One per window, beside `focus_thumb::FocusThumbnails`, and for the same
/// reason: a window walks only its own tabs, so a second window's pages cost
/// this one nothing and closing a window takes its pictures with it.
///
/// **Keyed by [`LeafId`] since F1b′**, which is the other half of that sentence:
/// the store spans every tab this window holds, and a seat number is unique only
/// inside one of them.
#[derive(Debug, Default)]
pub struct WebThumbs {
    pages: BTreeMap<LeafId, Entry>,
    tickets: u64,
    stats: WebThumbStats,
}

impl WebThumbs {
    #[must_use]
    pub fn stats(&self) -> WebThumbStats {
        self.stats
    }

    /// This pane's last frame, if it has one.
    #[must_use]
    pub fn picture(&self, leaf: LeafId) -> Option<&Picture> {
        self.pages.get(&leaf)?.picture.as_ref()
    }

    /// **Which of these pages to photograph now.**
    ///
    /// The whole policy, as a pure function over facts the caller read off the
    /// engine, so that every refusal below is testable without a browser.
    /// `demands` is the web panes of the cards that are **visible in the
    /// column** — the caller has already applied `focus_thumb`'s first two
    /// gates, and a window that is not in focus mode never calls this at all.
    pub fn due(&mut self, demands: &[PageDemand], now: Instant) -> Vec<LeafId> {
        let mut asking = Vec::new();
        for demand in demands {
            let entry = self.pages.entry(demand.leaf).or_insert_with(|| Entry {
                url: demand.url.clone(),
                picture: None,
                serial: 0,
                asked: None,
                started: None,
                source: None,
                encoded: None,
            });
            // **A seat that navigated is a seat whose picture is of another
            // page.** Asked here as well as on `Committed`, because the two say
            // it at different moments and the cheaper of them is this one: a
            // restored seat whose first commit arrived before any card was
            // drawn has no outcome left for the store to have seen.
            if entry.url != demand.url {
                entry.url.clone_from(&demand.url);
                if entry.picture.take().is_some() {
                    entry.serial += 1;
                }
                entry.asked = None;
                entry.source = None;
                entry.encoded = None;
            }
            if demand.facts.closing {
                self.stats.skipped_closing += 1;
                continue;
            }
            if !demand.facts.committed {
                self.stats.skipped_blank += 1;
                continue;
            }
            // **The measured fact, refused before the ask.** A hidden WebView's
            // capture never completes, so this is not "a call that would have
            // been wasted" — it is a slot that would be held open until the
            // seat closed.
            if !demand.facts.on_glass {
                self.stats.skipped_hidden += 1;
                continue;
            }
            if demand.facts.capturing || entry.asked.is_some() {
                self.stats.skipped_in_flight += 1;
                continue;
            }
            if !usable_target(demand.target) {
                self.stats.skipped_blank += 1;
                continue;
            }
            // **The clock, and the one thing that outruns it.** A pane that has
            // changed shape since its picture was taken is owed a new one at
            // once: the old pixels are a true picture of a layout that is no
            // longer there, and unlike every other kind of card this one cannot
            // simply be re-projected from memory.
            let reshaped = entry.picture.is_some() && entry.source != demand.facts.size;
            if !reshaped
                && let Some(started) = entry.started
                && now.duration_since(started) < CAPTURE_INTERVAL
            {
                self.stats.skipped_throttled += 1;
                continue;
            }
            self.tickets += 1;
            entry.asked = Some(Asked {
                ticket: self.tickets,
                url: demand.url.clone(),
                target: demand.target,
            });
            entry.started = Some(now);
            self.stats.captures += 1;
            asking.push(demand.leaf);
        }
        asking
    }

    /// **A capture came back.** Hand the bytes on to be shrunk, or throw them
    /// away.
    ///
    /// `url` is the page's identity *now*: a page that navigated between the ask
    /// and the answer has been photographed as something else, and the honest
    /// thing to do with those pixels is nothing.
    pub fn arrived(
        &mut self,
        leaf: LeafId,
        url: &str,
        png: Option<Vec<u8>>,
        source: Option<(u32, u32)>,
    ) -> Option<ShrinkJob> {
        let entry = self.pages.get_mut(&leaf)?;
        let asked = entry.asked.take()?;
        let Some(png) = png else {
            // The engine refused. The slot is released and the pane keeps
            // whatever it had; the clock still ran, so the next ask waits its
            // turn rather than hammering an engine that just said no.
            return None;
        };
        if asked.url != url || entry.url != url {
            self.stats.dropped_stale += 1;
            return None;
        }
        // **Gate 3, for the one card that had to be photographed to be asked.**
        // A page that has not changed hands back the same encoding, and the 18 ms
        // decode plus the megabyte upload behind it are refused here.
        if entry.encoded.as_deref() == Some(png.as_slice()) && entry.picture.is_some() {
            self.stats.skipped_unchanged += 1;
            entry.source = source;
            return None;
        }
        entry.encoded = Some(png.clone());
        entry.source = source;
        Some(ShrinkJob {
            leaf,
            ticket: asked.ticket,
            png,
            target: asked.target,
        })
    }

    /// **A shrunk picture came back from the worker.** Returns whether anything
    /// changed, so a caller can skip a repaint it does not owe.
    pub fn settle(&mut self, shrunk: ShrunkPicture) -> bool {
        let Some(entry) = self.pages.get_mut(&shrunk.leaf) else {
            return false;
        };
        // **The ticket is the whole of the staleness check.** A page that
        // navigated or was invalidated between the ask and this answer has had
        // its ask cleared and its serial moved, and a picture arriving for a
        // ticket nobody is holding is a picture of a page that is not there.
        if entry.ticket_is_stale(shrunk.ticket) {
            self.stats.dropped_stale += 1;
            return false;
        }
        let Some((rgba, width_px, height_px)) = shrunk.rgba else {
            return false;
        };
        entry.serial += 1;
        entry.picture = Some(Picture {
            // The tab is in the key as well as the seat since F1b′: two tabs
            // number their seats from one apiece, and a texture cache handed one
            // name for two pages draws the first page's pixels on the second
            // page's card.
            key: format!(
                "web-thumb:{}:{}:{}",
                shrunk.leaf.tab.0, shrunk.leaf.seat.0, entry.serial
            ),
            rgba: Arc::from(rgba),
            width_px,
            height_px,
        });
        self.stats.pictures += 1;
        true
    }

    /// **This pane is on a different page now** — drop what it was showing.
    ///
    /// Called on `WebOutcome::Committed`, which is the one event that says the
    /// identity moved. It cancels the ask in flight as well, so the picture that
    /// ask produces cannot land on the new page's slot.
    pub fn invalidate(&mut self, leaf: LeafId) -> bool {
        let Some(entry) = self.pages.get_mut(&leaf) else {
            return false;
        };
        let held = entry.picture.take().is_some();
        entry.asked = None;
        entry.source = None;
        entry.encoded = None;
        if held {
            entry.serial += 1;
        }
        held
    }

    /// Forget every pane that is not in this window's web map any more.
    ///
    /// The picture belongs to the pane, so a pane that has gone takes its
    /// picture with it — which is also the only place the memory is released.
    pub fn retain(&mut self, live: &BTreeSet<LeafId>) {
        self.pages.retain(|leaf, _| live.contains(leaf));
    }

    /// **Hand this pane's whole record over** — a tab moving to another window
    /// (Folio F1b), and the only reason a picture ever leaves one of these
    /// without being dropped.
    ///
    /// The *whole* entry and not just the picture: an ask in flight, the serial
    /// that dates it and the page it was of are all facts about this pane, and
    /// leaving any of them behind would let the answer to an outstanding ask land
    /// in a window the page has left. The v3 增补's rule for the frame itself is
    /// settled elsewhere and is untouched by this — the entry travels, and
    /// whether the frame it carries is still current is the caller's question.
    ///
    /// The key travels unchanged, because a tab keeps its `TabId` across a move
    /// (F1b) and a seat keeps its number inside that tab.
    #[allow(dead_code, reason = "F1c's drag and F2's menu row press the transfer")]
    #[must_use]
    pub fn take(&mut self, leaf: LeafId) -> Option<Entry> {
        self.pages.remove(&leaf)
    }

    /// [`Self::take`]'s other half, on the window the tab arrived in.
    #[allow(dead_code, reason = "F1c's drag and F2's menu row press the transfer")]
    pub fn put(&mut self, leaf: LeafId, entry: Entry) {
        self.pages.insert(leaf, entry);
    }
}

impl Entry {
    fn ticket_is_stale(&self, ticket: u64) -> bool {
        // The ask was taken by `arrived`, so the entry no longer names the
        // ticket; what says it is still wanted is that nothing has been asked
        // *since*. A newer ask means this answer is two generations old.
        self.asked
            .as_ref()
            .is_some_and(|asked| asked.ticket != ticket)
    }
}

/// Whether a card cell is worth photographing a page for.
fn usable_target(target: (u32, u32)) -> bool {
    target.0 >= 1 && target.1 >= 1 && target.0.saturating_mul(target.1) <= MAX_PICTURE_PIXELS
}

// ── The worker ─────────────────────────────────────────────────────────────

/// **Where the decode happens, which is nowhere near the frame.**
///
/// Gate 11 measured a page-sized PNG at 3.8 ms to decode and 14.3 ms to resample
/// — 18 ms, six times the whole projection budget, for one picture. It is a
/// thread and not a thread pool because the arrival rate is bounded by
/// [`CAPTURE_INTERVAL`] and by one capture in flight per seat: at most a handful
/// of jobs a second, each 18 ms, on one core.
pub struct PageShrinker {
    jobs: mpsc::Sender<ShrinkJob>,
    done: mpsc::Receiver<ShrunkPicture>,
}

impl std::fmt::Debug for PageShrinker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PageShrinker")
    }
}

impl PageShrinker {
    /// Start the worker. `wake` is called after every finished picture so that
    /// an idle window comes back for it — the same contract the web host's own
    /// `wake` has, and for the same reason.
    #[must_use]
    pub fn start(wake: impl Fn() + Send + 'static) -> Self {
        let (jobs, inbox) = mpsc::channel::<ShrinkJob>();
        let (finished, done) = mpsc::channel::<ShrunkPicture>();
        std::thread::Builder::new()
            .name("folio-web-thumb".to_owned())
            .spawn(move || {
                for job in inbox {
                    let rgba = shrink(&job.png, job.target);
                    if finished
                        .send(ShrunkPicture {
                            leaf: job.leaf,
                            ticket: job.ticket,
                            rgba,
                        })
                        .is_err()
                    {
                        return;
                    }
                    wake();
                }
            })
            .expect("the thumbnail shrinker thread");
        Self { jobs, done }
    }

    pub fn send(&self, job: ShrinkJob) {
        // A worker that has gone is a window that is closing; there is nothing
        // to report it to and nothing that would read the answer.
        let _ = self.jobs.send(job);
    }

    /// Everything the worker has finished since the last asking.
    pub fn collect(&self) -> Vec<ShrunkPicture> {
        self.done.try_iter().collect()
    }
}

/// **Decode a captured page and resample it to the box a card draws it in.**
///
/// A free function so that it can be tested without a thread, and so that the
/// one expensive thing this module does has a name a profile can point at.
///
/// `None` for bytes that will not decode: the engine handed over something this
/// build cannot read, which is a fact about the answer and not a reason to
/// invent pixels.
#[must_use]
pub fn shrink(png: &[u8], target: (u32, u32)) -> Option<(Vec<u8>, u32, u32)> {
    if !usable_target(target) {
        return None;
    }
    let decoded = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    // `Triangle` and not `Lanczos3`: this is a downscale by a factor of four or
    // more into a box 263 pixels wide, where the difference between the two
    // filters is not visible and the difference in cost is threefold. The
    // picture lane for a *file* picks Lanczos3 because that surface is looked at
    // full size and zoomed into; a card is neither.
    let small = image::imageops::resize(
        &decoded.to_rgba8(),
        target.0,
        target.1,
        image::imageops::FilterType::Triangle,
    );
    Some((small.into_raw(), target.0, target.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One pane, in a tab of its own — the shape every case below but
    /// [`one_seat_number_in_two_tabs_is_two_pictures`] is about.
    fn seat(id: u64) -> LeafId {
        leaf(1, id)
    }

    fn leaf(tab: u64, seat: u64) -> LeafId {
        LeafId {
            tab: crate::TabId(tab),
            seat: bt_layout::SeatId(seat),
        }
    }

    fn facts() -> SeatFacts {
        SeatFacts {
            on_glass: true,
            closing: false,
            committed: true,
            capturing: false,
            size: Some((1200, 800)),
        }
    }

    fn demand(id: u64, facts: SeatFacts) -> PageDemand {
        demand_for(seat(id), facts)
    }

    fn demand_for(leaf: LeafId, facts: SeatFacts) -> PageDemand {
        PageDemand {
            leaf,
            url: String::from("http://127.0.0.1:8080/"),
            facts,
            target: (263, 320),
        }
    }

    /// A one-pixel-per-cell PNG the shrinker can actually decode.
    fn a_real_png(width: u32, height: u32) -> Vec<u8> {
        let mut buffer = image::RgbaImage::new(width, height);
        for (x, y, pixel) in buffer.enumerate_pixels_mut() {
            *pixel = image::Rgba([(x % 256) as u8, (y % 256) as u8, 0x20, 0xff]);
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("encoding a test picture");
        bytes.into_inner()
    }

    /// Red gate: **a page that is not on the glass is never asked**, because a
    /// hidden WebView's capture does not come back — the ask would be a slot
    /// held open until the seat closed.
    #[test]
    fn a_page_that_is_not_on_the_glass_is_never_asked() {
        let mut thumbs = WebThumbs::default();
        let now = Instant::now();
        let asked = thumbs.due(
            &[demand(
                1,
                SeatFacts {
                    on_glass: false,
                    ..facts()
                },
            )],
            now,
        );
        assert!(asked.is_empty(), "asked a hidden page for a picture");
        assert_eq!(thumbs.stats().skipped_hidden, 1);
        assert_eq!(thumbs.stats().captures, 0);
    }

    /// Red gate: **a closing seat is never asked.** Its controller is gone and
    /// the wait for the browser runs for as long as ten seconds.
    #[test]
    fn a_closing_seat_is_never_asked() {
        let mut thumbs = WebThumbs::default();
        let asked = thumbs.due(
            &[demand(
                1,
                SeatFacts {
                    closing: true,
                    ..facts()
                },
            )],
            Instant::now(),
        );
        assert!(asked.is_empty(), "asked a closing seat for a picture");
        assert_eq!(thumbs.stats().skipped_closing, 1);
    }

    /// Red gate: **a seat that has committed nothing is never asked.** What is
    /// on its glass is the blank page the host minted for itself.
    #[test]
    fn a_seat_that_has_committed_nothing_is_never_asked() {
        let mut thumbs = WebThumbs::default();
        let asked = thumbs.due(
            &[demand(
                1,
                SeatFacts {
                    committed: false,
                    ..facts()
                },
            )],
            Instant::now(),
        );
        assert!(asked.is_empty(), "asked a blank seat for a picture");
        assert_eq!(thumbs.stats().skipped_blank, 1);
    }

    /// Red gate: one capture at a time per seat — the ask costs 0.115 ms and the
    /// answer takes 84 ms, so a second ask before the first lands is a queue
    /// growing at the frame rate.
    #[test]
    fn a_seat_with_a_capture_out_is_not_asked_again() {
        let mut thumbs = WebThumbs::default();
        let now = Instant::now();
        assert_eq!(thumbs.due(&[demand(1, facts())], now), vec![seat(1)]);
        let again = thumbs.due(&[demand(1, facts())], now + CAPTURE_INTERVAL * 3);
        assert!(again.is_empty(), "asked twice with one answer outstanding");
        assert_eq!(thumbs.stats().skipped_in_flight, 1);
    }

    /// Red gate: the clock. One picture per seat per [`CAPTURE_INTERVAL`], and
    /// the interval runs from the **ask** so a slow engine is not asked more
    /// often than a fast one.
    #[test]
    fn a_seat_is_photographed_at_most_once_an_interval() {
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        assert_eq!(thumbs.due(&[demand(1, facts())], start), vec![seat(1)]);
        let job = thumbs
            .arrived(
                seat(1),
                "http://127.0.0.1:8080/",
                Some(a_real_png(40, 30)),
                Some((1200, 800)),
            )
            .expect("the answer to the first ask");
        assert!(thumbs.settle(ShrunkPicture {
            leaf: seat(1),
            ticket: job.ticket,
            rgba: Some((vec![0; 263 * 320 * 4], 263, 320)),
        }));
        assert!(
            thumbs
                .due(
                    &[demand(1, facts())],
                    start + CAPTURE_INTERVAL - Duration::from_millis(1)
                )
                .is_empty(),
            "photographed again inside the interval"
        );
        assert_eq!(thumbs.stats().skipped_throttled, 1);
        assert_eq!(
            thumbs.due(&[demand(1, facts())], start + CAPTURE_INTERVAL),
            vec![seat(1)],
            "the interval passed and no second picture was taken"
        );
    }

    /// Red gate: **a navigation voids the picture.** The card must not go on
    /// showing the page the tab used to be on.
    #[test]
    fn a_committed_navigation_voids_the_picture() {
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        thumbs.due(&[demand(1, facts())], start);
        let job = thumbs
            .arrived(
                seat(1),
                "http://127.0.0.1:8080/",
                Some(a_real_png(40, 30)),
                Some((1200, 800)),
            )
            .expect("the answer");
        thumbs.settle(ShrunkPicture {
            leaf: seat(1),
            ticket: job.ticket,
            rgba: Some((vec![0; 263 * 320 * 4], 263, 320)),
        });
        let before = thumbs.picture(seat(1)).expect("a picture").key.clone();
        assert!(thumbs.invalidate(seat(1)));
        assert!(
            thumbs.picture(seat(1)).is_none(),
            "the card still holds a picture of the page it left"
        );
        // And the *next* picture is a new identity, so the texture cache cannot
        // hand back the one that was just voided.
        thumbs.due(&[demand(1, facts())], start + CAPTURE_INTERVAL);
        let job = thumbs
            .arrived(
                seat(1),
                "http://127.0.0.1:8080/",
                Some(a_real_png(40, 30)),
                Some((1200, 800)),
            )
            .expect("the second answer");
        thumbs.settle(ShrunkPicture {
            leaf: seat(1),
            ticket: job.ticket,
            rgba: Some((vec![0; 263 * 320 * 4], 263, 320)),
        });
        assert_ne!(
            thumbs.picture(seat(1)).expect("a picture").key,
            before,
            "the replacement wears the voided picture's identity"
        );
    }

    /// Red gate: the same, said by the *demand* rather than by the outcome — a
    /// seat whose identity changed under the store has its picture dropped on
    /// the next pass even if nobody called [`WebThumbs::invalidate`].
    #[test]
    fn a_seat_that_is_on_another_url_loses_its_picture_on_the_next_pass() {
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        thumbs.due(&[demand(1, facts())], start);
        let job = thumbs
            .arrived(
                seat(1),
                "http://127.0.0.1:8080/",
                Some(a_real_png(40, 30)),
                Some((1200, 800)),
            )
            .expect("the answer");
        thumbs.settle(ShrunkPicture {
            leaf: seat(1),
            ticket: job.ticket,
            rgba: Some((vec![0; 263 * 320 * 4], 263, 320)),
        });
        let mut moved = demand(1, facts());
        moved.url = String::from("http://127.0.0.1:8080/other");
        thumbs.due(&[moved], start + CAPTURE_INTERVAL);
        assert!(
            thumbs.picture(seat(1)).is_none(),
            "a card kept a picture of another page"
        );
    }

    /// Red gate: an answer that arrives after the seat has moved on is thrown
    /// away rather than drawn.
    #[test]
    fn an_answer_that_arrives_after_the_page_moved_on_is_thrown_away() {
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        thumbs.due(&[demand(1, facts())], start);
        let dropped = thumbs.arrived(
            seat(1),
            "http://127.0.0.1:8080/other",
            Some(a_real_png(40, 30)),
            Some((1200, 800)),
        );
        assert!(dropped.is_none(), "a picture of the old page was accepted");
        assert_eq!(thumbs.stats().dropped_stale, 1);
    }

    /// Red gate: a pane that changed shape is owed a picture at once — the old
    /// pixels are a true picture of a layout that is no longer there, and a page
    /// is the one kind of card that cannot be re-projected from memory.
    #[test]
    fn a_reshaped_pane_is_photographed_without_waiting_for_the_clock() {
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        thumbs.due(&[demand(1, facts())], start);
        let job = thumbs
            .arrived(
                seat(1),
                "http://127.0.0.1:8080/",
                Some(a_real_png(40, 30)),
                Some((1200, 800)),
            )
            .expect("the answer");
        thumbs.settle(ShrunkPicture {
            leaf: seat(1),
            ticket: job.ticket,
            rgba: Some((vec![0; 263 * 320 * 4], 263, 320)),
        });
        let reshaped = demand(
            1,
            SeatFacts {
                size: Some((900, 800)),
                ..facts()
            },
        );
        assert_eq!(
            thumbs.due(&[reshaped], start + Duration::from_millis(10)),
            vec![seat(1)],
            "a pane that changed shape waited two seconds to be re-photographed"
        );
    }

    /// Red gate: the picture belongs to the **seat**, not to the card. A page
    /// that has gone off the glass keeps the frame it last stood there with,
    /// because there is no other way it could ever have one again.
    #[test]
    fn a_page_that_left_the_glass_keeps_its_last_frame() {
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        thumbs.due(&[demand(1, facts())], start);
        let job = thumbs
            .arrived(
                seat(1),
                "http://127.0.0.1:8080/",
                Some(a_real_png(40, 30)),
                Some((1200, 800)),
            )
            .expect("the answer");
        thumbs.settle(ShrunkPicture {
            leaf: seat(1),
            ticket: job.ticket,
            rgba: Some((vec![0; 263 * 320 * 4], 263, 320)),
        });
        let key = thumbs.picture(seat(1)).expect("a picture").key.clone();
        thumbs.due(
            &[demand(
                1,
                SeatFacts {
                    on_glass: false,
                    ..facts()
                },
            )],
            start + CAPTURE_INTERVAL * 5,
        );
        assert_eq!(
            thumbs.picture(seat(1)).map(|picture| picture.key.clone()),
            Some(key),
            "a page that went into the background lost the only picture it can have"
        );
    }

    /// PIN (F1b′) — **one seat number in two tabs is two pictures.**
    ///
    /// Two panes, both called seat 1, both on the same URL, in the two tabs of
    /// one window. Keyed by the seat number alone they were one slot, and the
    /// consequences compounded: the second demand found the first's entry, the
    /// two took turns being throttled by each other's clock, and whichever card
    /// drew second drew the other tab's page — with the same texture key, so the
    /// cache had no way to tell them apart either.
    ///
    /// MUTATION: key [`WebThumbs::pages`] by `leaf.seat` and the first
    /// assertion answers one slot, the second answers `vec![]` because the
    /// second pane is throttled by the first one's clock, and the last two
    /// answer the same texture key.
    #[test]
    fn one_seat_number_in_two_tabs_is_two_pictures() {
        let first = leaf(1, 1);
        let second = leaf(2, 1);
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        assert_eq!(
            thumbs.due(
                &[demand_for(first, facts()), demand_for(second, facts())],
                start
            ),
            vec![first, second],
            "both panes are asked, because they are two pages and not one"
        );
        for (leaf, pixels) in [(first, 0x11), (second, 0x22)] {
            let job = thumbs
                .arrived(leaf, "http://127.0.0.1:8080/", Some(vec![pixels; 8]), None)
                .expect("a job for each");
            assert_eq!(job.leaf, leaf);
            thumbs.settle(ShrunkPicture {
                leaf,
                ticket: job.ticket,
                rgba: Some((vec![pixels; 263 * 320 * 4], 263, 320)),
            });
        }
        let keys = [first, second].map(|leaf| {
            thumbs
                .picture(leaf)
                .expect("each pane kept its own picture")
                .key
                .clone()
        });
        assert_ne!(
            keys[0], keys[1],
            "and the two pictures are two entries in the texture cache, or the \
             second card draws the first tab's page"
        );
        assert!(
            thumbs.invalidate(second) && thumbs.picture(first).is_some(),
            "a navigation in one tab does not blank the other tab's card"
        );
    }

    /// Red gate: a seat that has gone takes its picture with it — the one place
    /// the pixels are released.
    #[test]
    fn a_seat_that_went_away_takes_its_picture_with_it() {
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        thumbs.due(&[demand(1, facts()), demand(2, facts())], start);
        thumbs.retain(&BTreeSet::from([seat(1)]));
        assert!(thumbs.picture(seat(2)).is_none());
        // And it is gone rather than merely blank: the next pass mints a fresh
        // slot for it, which is what releases the pixels.
        assert_eq!(
            thumbs.due(&[demand(2, facts())], start + CAPTURE_INTERVAL),
            vec![seat(2)]
        );
    }

    /// Red gate: the picture's identity is the damage key, and it moves **only**
    /// when the picture is replaced. A card asked sixty times a second about a
    /// page that has not been re-photographed re-projects nothing.
    #[test]
    fn the_identity_stands_still_while_the_picture_does() {
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        thumbs.due(&[demand(1, facts())], start);
        let job = thumbs
            .arrived(
                seat(1),
                "http://127.0.0.1:8080/",
                Some(a_real_png(40, 30)),
                Some((1200, 800)),
            )
            .expect("the answer");
        thumbs.settle(ShrunkPicture {
            leaf: seat(1),
            ticket: job.ticket,
            rgba: Some((vec![0; 263 * 320 * 4], 263, 320)),
        });
        let key = thumbs.picture(seat(1)).expect("a picture").key.clone();
        for frame in 1..60 {
            thumbs.due(&[demand(1, facts())], start + Duration::from_millis(frame));
        }
        assert_eq!(
            thumbs.picture(seat(1)).expect("a picture").key,
            key,
            "an untouched picture asked its card to re-project"
        );
    }

    /// Red gate: the shrinker really does produce the box it was asked for, and
    /// really does refuse bytes it cannot read.
    #[test]
    fn the_shrinker_answers_the_box_it_was_asked_for_and_refuses_what_it_cannot_read() {
        let (rgba, width, height) = shrink(&a_real_png(1146, 777), (263, 320)).expect("a picture");
        assert_eq!((width, height), (263, 320));
        assert_eq!(rgba.len(), 263 * 320 * 4);
        assert!(shrink(b"not a png at all", (263, 320)).is_none());
    }

    /// Red gate: **a page that has not changed is photographed and then let go**
    /// — no decode, no resample, no texture, and no re-projection of its card.
    ///
    /// This is `focus_thumb`'s gate 3 reaching the one kind of card that cannot
    /// be asked anything until it has been photographed. Without it a still page
    /// beside a scrolling shell would pay 18 ms of decode and a megabyte of
    /// upload every two seconds for a picture identical to the one already on
    /// screen.
    #[test]
    fn a_page_that_has_not_changed_costs_nothing_after_its_first_frame() {
        let mut thumbs = WebThumbs::default();
        let start = Instant::now();
        let still = a_real_png(40, 30);
        thumbs.due(&[demand(1, facts())], start);
        let job = thumbs
            .arrived(
                seat(1),
                "http://127.0.0.1:8080/",
                Some(still.clone()),
                Some((1200, 800)),
            )
            .expect("the first answer is always work");
        thumbs.settle(ShrunkPicture {
            leaf: seat(1),
            ticket: job.ticket,
            rgba: Some((vec![0; 263 * 320 * 4], 263, 320)),
        });
        let key = thumbs.picture(seat(1)).expect("a picture").key.clone();

        thumbs.due(&[demand(1, facts())], start + CAPTURE_INTERVAL);
        assert!(
            thumbs
                .arrived(
                    seat(1),
                    "http://127.0.0.1:8080/",
                    Some(still),
                    Some((1200, 800)),
                )
                .is_none(),
            "an unchanged page was sent to the decoder anyway"
        );
        assert_eq!(thumbs.stats().skipped_unchanged, 1);
        assert_eq!(
            thumbs.picture(seat(1)).expect("a picture").key,
            key,
            "and its card was asked to re-project for a picture it already had"
        );

        // A page that *did* change is still work, which is what stops this from
        // being a card frozen on its first frame.
        thumbs.due(&[demand(1, facts())], start + CAPTURE_INTERVAL * 2);
        assert!(
            thumbs
                .arrived(
                    seat(1),
                    "http://127.0.0.1:8080/",
                    Some(a_real_png(41, 30)),
                    Some((1200, 800)),
                )
                .is_some(),
            "a page that moved was refused as if it had not"
        );
    }

    /// **The page lane's own budget** (W2 slice ⑥ against §7.1.6b′ F2's
    /// `FULL_BLAST_BUDGET_MS = 3.0`).
    ///
    /// The projection budget's own test is untouched by this slice and stays
    /// green as it stands; what this adds is the second pass the same frame now
    /// makes. The shape is the projection budget's own — ten tabs, two seats
    /// each — with **every one of the twenty a page**, which is far past
    /// anything the product can be asked for: a tab holds one page per seat and
    /// a column of twenty web seats would be twenty browsers.
    ///
    /// What is being bounded is the *decision*, which is all that happens on the
    /// frame: the ask itself is one syscall measured at 0.115 ms and made at most
    /// once every [`CAPTURE_INTERVAL`] per seat, and the decode happens on
    /// [`PageShrinker`]'s thread. So the frame is charged the walk plus, twice a
    /// second, one syscall — and the number below is the walk.
    #[test]
    fn the_page_lane_stays_far_inside_the_projection_budget() {
        /// The ceiling `focus_thumb`'s own budget test states, quoted rather
        /// than re-chosen: this lane runs inside the same frame, so it is the
        /// same three milliseconds being spent.
        const FULL_BLAST_BUDGET_MS: f64 = 3.0;
        let start = Instant::now();
        let demands: Vec<PageDemand> = (0..20)
            .map(|id| PageDemand {
                leaf: seat(id),
                url: format!("http://127.0.0.1:8080/page-{id}"),
                facts: facts(),
                target: (263, 320),
            })
            .collect();
        let mut thumbs = WebThumbs::default();
        let mut samples = Vec::with_capacity(200);
        for tick in 0..200 {
            let now = start + Duration::from_millis(8 * tick);
            let began = Instant::now();
            thumbs.due(&demands, now);
            samples.push(began.elapsed().as_secs_f64() * 1_000.0);
        }
        samples.sort_by(f64::total_cmp);
        let observed = samples[samples.len() / 2];
        println!("W2 slice 6 page lane: {observed:.4}ms median per frame");
        assert!(
            observed <= FULL_BLAST_BUDGET_MS,
            "twenty page seats cost {observed:.4}ms a frame to decide about, \
             over the {FULL_BLAST_BUDGET_MS}ms the whole column is budgeted"
        );
        // And the decision really is a decision: with a capture out on every
        // seat from the first frame, the other 199 refuse without work.
        assert_eq!(thumbs.stats().captures, 20);
        assert!(thumbs.stats().skipped_in_flight >= 20 * 199);
    }

    /// Red gate: a box the column could not possibly be asking for is refused
    /// before a browser is troubled for a picture.
    #[test]
    fn an_impossible_box_is_refused_before_the_engine_is_asked() {
        let mut thumbs = WebThumbs::default();
        let mut huge = demand(1, facts());
        huge.target = (4000, 4000);
        assert!(thumbs.due(&[huge], Instant::now()).is_empty());
        let mut nothing = demand(2, facts());
        nothing.target = (0, 40);
        assert!(thumbs.due(&[nothing], Instant::now()).is_empty());
    }
}
