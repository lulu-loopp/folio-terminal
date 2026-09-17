//! **When a drain turn's frame is published, and when it waits a moment for the rest.**
//!
//! A pty carries a byte stream and no record boundary, so a terminal is never told where a
//! program's repaint ends. On macOS the kernel hands over at most 1024 bytes per `read(2)`, so a
//! full-screen program's repaint of a few kilobytes arrives as three to eight reads with a
//! genuinely empty ring between them — and an empty ring is the only end-of-output signal the
//! drain has. Folio therefore composed and published a frame built from half of a repaint: a grid
//! on which a typeset formula's source is incomplete, so its picture is rightly withdrawn and its
//! torn source shown. Scrolling inside a TUI did that on every scroll.
//!
//! The remedy is not to guess where the frame ends. It is to notice that the *kernel already
//! said* there was more — a read that returned the transport's whole transfer unit
//! ([`bt_pty::OutputSlice::ends_capped`]) — and to wait a bounded moment for it.
//!
//! **This is a rule about the transfer, not about the bytes.** Nothing here reads a byte, knows
//! what program is running, or treats the alternate screen differently from the primary. What
//! would not be sound — and what this deliberately is not — is inferring a frame boundary from
//! escape sequences (`CSI ?25l`/`h`, `ED`, `CUP 1;1`): those are conventions, and acting on them
//! is reading the application's mind. Measured on the owner's recording: of its 116 cursor-hidden
//! repaint brackets, **not one** had all of its interior read boundaries capped, so a terminal
//! that framed on those brackets would have been wrong 116 times out of 116.
//!
//! The deferral is bounded twice, and both bounds are what keep this a scheduling rule rather
//! than a bet: never past [`COALESCE_WINDOW`] after the **first** unpublished byte, so a flood
//! whose every read is capped still publishes at `1/T` and cannot be starved; and never past the
//! next display deadline, so the wait can only ever spend latency the display was going to spend.

use std::time::{Duration, Instant};

/// **How long a drain turn waits for the rest of a burst the kernel said was coming.**
///
/// Three milliseconds, and the number is measured rather than chosen. On the owner's macOS
/// recording of 2026-09-17 (350 reads of one Claude Code session, 123 of them the pty's
/// 1024-byte cap), the gap from a capped read to the next read was 1.1 ms at the median, and
/// **92 % of capped reads were followed within 2 ms and 98 % within 4 ms** — the rest of a
/// repaint is inside one 60 Hz frame. Replaying that recording through the session under this
/// policy, the torn-source events fell from 58 to 22 and stopped falling at T = 2–3 ms, which is
/// exactly where the gap census says the returns stop.
///
/// It is the same order as the two other terminals that solve this without the application's
/// cooperation (WezTerm's `mux_output_parser_coalesce_delay_ms`, kitty's `input_delay`, both 3 ms)
/// — arrived at independently, which is some comfort about the constant.
///
/// A constant and not a setting: it is a property of how a pty hands bytes over, not a taste.
pub(crate) const COALESCE_WINDOW: Duration = Duration::from_millis(3);

/// **The window's half of the rule: what it owes the glass, and when it must pay.**
///
/// **The invariant, and it is the whole of it: [`Self::until`] is never `Some` without a wake
/// booked for it.** It is folded into the event loop's `ControlFlow::WaitUntil` beside every
/// other deadline this window keeps, and `Runtime::finish_pty_coalesce_if_due` runs on every turn
/// and publishes and disarms the instant it passes — so a deadline cannot outlive the wait it
/// asked for, however the window is closed, hidden, resized or emptied in the meantime. A
/// deadline that outlived its wake would be a pane that silently stopped presenting, which is
/// the shape `bt_term::scheduling::ResizeEpoch` is written against; the two fields live in one
/// type and are cleared together so that there is one place to get that wrong rather than two.
///
/// Both fields are the *window's* and not a pane's, because publication is the window's:
/// `Runtime::publish_pty_drain_frame` composes one picture for the whole window. See
/// `DrainOutcome::arrived_uncapped` for what that means for a window whose panes disagree.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Pending {
    /// When the earliest byte this window has fed but not published arrived.
    pub first_unpublished: Option<Instant>,
    /// The instant a deferred publication must happen by, when one is deferred.
    pub until: Option<Instant>,
}

impl Pending {
    /// **Every publish settles the debt.** What was fed is on the glass — or was found to be no
    /// different from what already is, which answers the same question — so nothing is owed and
    /// nothing is waiting.
    pub(crate) fn settle(&mut self) {
        *self = Self::default();
    }

    /// Note that this window owes a picture, and answer when it started owing it.
    pub(crate) fn opened_at(&mut self, now: Instant) -> Instant {
        *self.first_unpublished.get_or_insert(now)
    }
}

/// What one drain turn does about the frame it has composed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Publication {
    /// Publish it now, exactly as every drain turn did before this rule existed.
    Now,
    /// Hold it until this instant, or until more bytes arrive — whichever comes first.
    WaitUntil(Instant),
}

/// Everything the decision is allowed to see. All of it is timing and transport state; none of it
/// is content.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arrival {
    /// The last slice this turn fed ended with a whole `read(2)` the transport had capped.
    pub ends_capped: bool,
    /// A pane's ring still holds bytes. The turn is coming straight back for them, so there is
    /// nothing to decide.
    pub ring_pending: bool,
    /// A DEC 2026 synchronized update is open on the pane that owns the picture. The parser is
    /// withholding the block's bytes, so a frame composed now cannot differ and a wait buys
    /// nothing at all.
    pub sync_open: bool,
    /// A DEC 2026 synchronized update **committed** during this turn. The program has just said,
    /// in the protocol, where its frame ends — which is better information than any timer — and
    /// making that frame wait behind one would delay a picture that was already correct.
    pub sync_closed: bool,
    /// When the earliest byte this window has fed but not yet published arrived. `None` when the
    /// window owes nothing, which is the ordinary resting state.
    pub first_unpublished: Option<Instant>,
    /// The next instant the display will take a frame, when the window knows of one. The wait is
    /// never allowed past it.
    pub next_display_deadline: Option<Instant>,
}

/// **The rule, as one pure function of the clock.**
///
/// `window` is [`COALESCE_WINDOW`] in the product and an argument here so that `T = 0` — which
/// must be provably the behaviour Folio had before this existed — is a case the tests can state.
pub(crate) fn decide(arrival: Arrival, now: Instant, window: Duration) -> Publication {
    // Nothing to wait for: either the kernel said that was all there was, or there are bytes in
    // hand already, or the program is bracketing its own repaint and needs no help from a timer.
    if !arrival.ends_capped
        || arrival.ring_pending
        || arrival.sync_open
        || arrival.sync_closed
        || window.is_zero()
    {
        return Publication::Now;
    }
    // Measured from the first unpublished byte and never from this read, so a burst of capped
    // reads a millisecond apart cannot push the deadline along in front of itself. This is the
    // whole of the flood bound: the wait ends `window` after the burst began, whatever arrives
    // in the meantime.
    let opened = arrival.first_unpublished.unwrap_or(now);
    let deadline = opened + window;
    let deadline = match arrival.next_display_deadline {
        Some(display) => deadline.min(display),
        None => deadline,
    };
    if deadline <= now {
        return Publication::Now;
    }
    Publication::WaitUntil(deadline)
}

#[cfg(test)]
mod session_tests {
    //! **The rule composed with a real terminal session**, on a stream written here rather than
    //! recorded from anyone: a typeset formula, then a repaint that arrives in four capped pieces
    //! a fraction of a millisecond apart, the way the macOS pty delivers one.
    //!
    //! The driver below is the shape of `Runtime::drain_pty` — feed, then decide, then publish or
    //! wait — reduced to the two things this is about. It lives here rather than in `bt-term`
    //! because the rule does: `bt-term` knows nothing of publication.

    use std::num::NonZeroU32;
    use std::time::{Duration, Instant};

    use bt_math::MathEngine;
    use bt_term::{
        DualPlaneSession, LIVE_MATH_STABLE_INTERVAL, SessionMathTask, observe_formula_frame,
        render_detection_task, render_live_detection_task,
    };
    use bt_viewport::ViewportProjection;

    use super::{Arrival, COALESCE_WINDOW, Publication, decide};

    const INK: [u8; 3] = [0xd8, 0xdc, 0xe8];
    /// Short enough to wrap nothing at this width — a longer one breaks over two rows and fakes
    /// the very failure this is measuring.
    const FORMULA: &str = "$$e^{i\\pi} + 1 = 0$$";

    struct Screen {
        session: DualPlaneSession,
        projection: ViewportProjection,
        engine: MathEngine,
    }

    impl Screen {
        fn new() -> Self {
            let session = DualPlaneSession::new(
                NonZeroU32::new(40).expect("a width"),
                NonZeroU32::new(12).expect("a height"),
            );
            let projection = session.new_projection(session.layout_key());
            Self {
                session,
                projection,
                engine: MathEngine::new(),
            }
        }

        fn feed(&mut self, bytes: &[u8], at: Instant) {
            self.session.feed_at(bytes, at).expect("a feed");
            self.render_whatever_was_asked_for();
        }

        /// The math worker, run in line. Off the thread it really lives on, a raster lands some
        /// milliseconds later; here it lands at once, which makes the frames deterministic and
        /// understates rather than overstates how long a withdrawn picture stays away.
        fn render_whatever_was_asked_for(&mut self) {
            while let Some(task) = self.session.take_math_worker_task() {
                match task {
                    SessionMathTask::Frozen(mut task) => {
                        let result = render_detection_task(&self.engine, &mut task, INK);
                        let _ = self.session.complete_worker_result(task, result);
                    }
                    SessionMathTask::Live(mut task) => {
                        let result = render_live_detection_task(&self.engine, &mut task, INK);
                        let _ = self.session.complete_live_worker_result(task, result);
                    }
                }
            }
        }

        fn tick(&mut self, at: Instant) {
            let _ = self.session.finish_resize_if_quiescent(at);
            self.session.advance_live_stability(at);
            self.render_whatever_was_asked_for();
        }

        /// One published frame, answered as the only question this test asks of it: is the
        /// formula a picture, or is its source lying there in the cells?
        fn published_frame_draws_the_formula(&mut self) -> bool {
            self.session.refresh_projection(&mut self.projection);
            let frame = self
                .session
                .viewport_frame(&mut self.projection)
                .expect("a frame");
            !observe_formula_frame(&frame).rendered_sources.is_empty()
        }
    }

    /// A full-screen program's repaint: erase, then rewrite, in four pieces the transport capped.
    fn repaint_pieces() -> Vec<Vec<u8>> {
        vec![
            b"\x1b[H\x1b[2Jline one\r\n".to_vec(),
            b"line two\r\n".to_vec(),
            format!("{FORMULA}\r\n").into_bytes(),
            b"line four\r\n".to_vec(),
        ]
    }

    /// Drive one split repaint under a given window and answer what each published frame drew.
    fn frames_published_over_a_split_repaint(window: Duration) -> Vec<bool> {
        let start = Instant::now();
        let mut screen = Screen::new();
        screen.feed(
            format!("\x1b[?1049h\x1b[H\x1b[2Jline one\r\nline two\r\n{FORMULA}\r\nline four\r\n")
                .as_bytes(),
            start,
        );
        let settled = start + LIVE_MATH_STABLE_INTERVAL;
        screen.tick(settled);
        assert!(
            screen.published_frame_draws_the_formula(),
            "the block has to be a picture before a repaint can take it away"
        );

        let mut drawn = Vec::new();
        let mut first_unpublished: Option<Instant> = None;
        let mut armed: Option<Instant> = None;
        let pieces = repaint_pieces();
        let last = pieces.len() - 1;
        for (step, piece) in pieces.iter().enumerate() {
            let at = settled + Duration::from_micros(500 * step as u64);
            if let Some(deadline) = armed.take_if(|deadline| *deadline <= at) {
                screen.tick(deadline);
                drawn.push(screen.published_frame_draws_the_formula());
                first_unpublished = None;
            }
            screen.feed(piece, at);
            first_unpublished.get_or_insert(at);
            armed = match decide(
                Arrival {
                    // Every piece is a whole read the transport capped but the last, which is
                    // the kernel saying the repaint is over.
                    ends_capped: step < last,
                    ring_pending: false,
                    sync_open: false,
                    sync_closed: false,
                    first_unpublished,
                    next_display_deadline: None,
                },
                at,
                window,
            ) {
                Publication::Now => {
                    drawn.push(screen.published_frame_draws_the_formula());
                    first_unpublished = None;
                    None
                }
                Publication::WaitUntil(deadline) => Some(deadline),
            };
        }
        if let Some(deadline) = armed {
            screen.tick(deadline);
            drawn.push(screen.published_frame_draws_the_formula());
        }
        assert!(!drawn.is_empty(), "a repaint publishes something");
        drawn
    }

    /// **The defect, stated.** With no window at all — the behaviour Folio had — a frame composed
    /// between two pieces of one repaint reaches the glass with the picture gone and its source
    /// lying in the cells. This is the red evidence the rule is for.
    #[test]
    fn without_a_window_a_split_repaint_publishes_the_torn_frame() {
        let drawn = frames_published_over_a_split_repaint(Duration::ZERO);
        assert!(
            drawn.iter().any(|shown| !shown),
            "publishing after every read shows the repaint half-done: {drawn:?}"
        );
    }

    /// **The repair.** The same four pieces, the same instants, one bounded wait: every frame
    /// that reaches the glass draws the formula, and none of them draws its source.
    #[test]
    fn a_bounded_window_publishes_only_whole_repaints() {
        let drawn = frames_published_over_a_split_repaint(COALESCE_WINDOW);
        assert!(
            drawn.iter().all(|shown| *shown),
            "no published frame may show a repaint half-done: {drawn:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival() -> Arrival {
        Arrival {
            ends_capped: true,
            ring_pending: false,
            sync_open: false,
            sync_closed: false,
            first_unpublished: None,
            next_display_deadline: None,
        }
    }

    fn millis(count: u64) -> Duration {
        Duration::from_millis(count)
    }

    /// A publish settles everything the wait was about; a debt, once opened, keeps the instant it
    /// opened at however many turns of bytes land on top of it.
    #[test]
    fn a_publish_settles_the_debt_and_a_debt_keeps_its_opening_instant() {
        let opened = Instant::now();
        let mut pending = Pending::default();
        assert_eq!(pending.opened_at(opened), opened);
        assert_eq!(pending.opened_at(opened + millis(2)), opened);
        pending.until = Some(opened + COALESCE_WINDOW);

        pending.settle();
        assert_eq!(pending, Pending::default());
        assert!(
            pending.until.is_none(),
            "a settled window owes nothing and waits for nothing"
        );
        // And the next burst opens where it opens, not where the last one did.
        let later = opened + millis(50);
        assert_eq!(pending.opened_at(later), later);
    }

    /// A keystroke's echo is a short read — the kernel had nothing more — and it is published on
    /// the turn it arrived, with nothing added. This is the latency claim in one line.
    #[test]
    fn a_short_read_is_published_now() {
        let now = Instant::now();
        let echo = Arrival {
            ends_capped: false,
            first_unpublished: Some(now),
            ..arrival()
        };
        assert_eq!(decide(echo, now, COALESCE_WINDOW), Publication::Now);
    }

    #[test]
    fn a_capped_read_with_a_dry_ring_waits_from_the_first_unpublished_byte() {
        let opened = Instant::now();
        let now = opened + millis(1);
        let waiting = Arrival {
            first_unpublished: Some(opened),
            ..arrival()
        };
        assert_eq!(
            decide(waiting, now, COALESCE_WINDOW),
            Publication::WaitUntil(opened + COALESCE_WINDOW)
        );
    }

    /// The flood bound. Eight capped reads a millisecond apart under a three-millisecond window
    /// publish three times — once every window and not once per read — and the last read's
    /// arrival never moves the deadline it finds.
    /// **The flood bound, run as the loop runs it.** Eight capped reads a millisecond apart under
    /// a three-millisecond window: the picture reaches the glass every window and not once per
    /// read, and no byte ever waits longer than the window — which is what "cannot be starved"
    /// means for a pane printing without pause.
    #[test]
    fn a_burst_of_capped_reads_publishes_once_every_window() {
        let start = Instant::now();
        let reads = (0..8_u64)
            .map(|step| start + millis(step))
            .collect::<Vec<_>>();
        let window = millis(3);

        let mut first_unpublished: Option<Instant> = None;
        let mut armed: Option<Instant> = None;
        let mut published: Vec<Instant> = Vec::new();
        let mut worst_wait = Duration::ZERO;
        let mut publish = |at: Instant, first: &mut Option<Instant>, worst: &mut Duration| {
            if let Some(opened) = first.take() {
                *worst = (*worst).max(at.saturating_duration_since(opened));
            }
            published.push(at);
        };

        for read in &reads {
            // The wake the deadline booked, when it falls before the next bytes.
            if let Some(deadline) = armed.take_if(|deadline| *deadline <= *read) {
                publish(deadline, &mut first_unpublished, &mut worst_wait);
            }
            first_unpublished.get_or_insert(*read);
            armed = match decide(
                Arrival {
                    first_unpublished,
                    ..arrival()
                },
                *read,
                window,
            ) {
                Publication::Now => {
                    publish(*read, &mut first_unpublished, &mut worst_wait);
                    None
                }
                Publication::WaitUntil(deadline) => Some(deadline),
            };
        }
        // The burst stops; the last wake pays for what it left.
        if let Some(deadline) = armed {
            publish(deadline, &mut first_unpublished, &mut worst_wait);
        }

        assert_eq!(
            published.len(),
            3,
            "a flood publishes every window, not every read: {published:?}"
        );
        for pair in published.windows(2) {
            assert!(
                pair[1].saturating_duration_since(pair[0]) <= window,
                "a flood must reach the glass at least every window"
            );
        }
        assert!(
            worst_wait <= window,
            "no byte waits longer than the window: {worst_wait:?}"
        );
    }

    /// Within one burst the deadline is set once and never moves later, however many capped reads
    /// land behind it.
    #[test]
    fn the_deadline_never_moves_later_within_a_burst() {
        let opened = Instant::now();
        let waiting = Arrival {
            first_unpublished: Some(opened),
            ..arrival()
        };
        let first = decide(waiting, opened, COALESCE_WINDOW);
        let later = decide(waiting, opened + millis(2), COALESCE_WINDOW);
        assert_eq!(first, later);
        assert_eq!(first, Publication::WaitUntil(opened + COALESCE_WINDOW));
    }

    /// A window that has already run out publishes rather than asking for a wake in the past —
    /// the rule the loop's deadline fold is pinned on.
    #[test]
    fn a_window_that_has_run_out_publishes_now() {
        let opened = Instant::now();
        let waiting = Arrival {
            first_unpublished: Some(opened),
            ..arrival()
        };
        assert_eq!(
            decide(waiting, opened + COALESCE_WINDOW, COALESCE_WINDOW),
            Publication::Now
        );
    }

    /// A program that brackets its own repaint has already made it atomic. Adding a timer on top
    /// would delay a frame that was going to be correct.
    #[test]
    fn an_open_synchronized_update_is_published_now() {
        let now = Instant::now();
        let bracketed = Arrival {
            sync_open: true,
            first_unpublished: Some(now),
            ..arrival()
        };
        assert_eq!(decide(bracketed, now, COALESCE_WINDOW), Publication::Now);
    }

    /// The ESU is the program naming its own frame boundary. That frame goes out at once.
    #[test]
    fn a_synchronized_update_that_committed_is_published_now() {
        let now = Instant::now();
        let committed = Arrival {
            sync_closed: true,
            first_unpublished: Some(now),
            ..arrival()
        };
        assert_eq!(decide(committed, now, COALESCE_WINDOW), Publication::Now);
    }

    /// Bytes already in hand are not waited for: the turn is coming straight back for them.
    #[test]
    fn a_ring_that_still_holds_bytes_is_published_now() {
        let now = Instant::now();
        let more = Arrival {
            ring_pending: true,
            first_unpublished: Some(now),
            ..arrival()
        };
        assert_eq!(decide(more, now, COALESCE_WINDOW), Publication::Now);
    }

    /// The wait never outlives the frame it is waiting for.
    #[test]
    fn the_display_deadline_cuts_the_wait_short() {
        let opened = Instant::now();
        let display = opened + millis(1);
        let waiting = Arrival {
            first_unpublished: Some(opened),
            next_display_deadline: Some(display),
            ..arrival()
        };
        assert_eq!(
            decide(waiting, opened, COALESCE_WINDOW),
            Publication::WaitUntil(display)
        );
    }

    /// **`T = 0` is the behaviour Folio had before this rule existed**, for every input there is.
    #[test]
    fn a_zero_window_is_the_old_behaviour() {
        let now = Instant::now();
        for ends_capped in [false, true] {
            for ring_pending in [false, true] {
                for sync_open in [false, true] {
                    for sync_closed in [false, true] {
                        for first_unpublished in [None, Some(now - millis(5))] {
                            let case = Arrival {
                                ends_capped,
                                ring_pending,
                                sync_open,
                                sync_closed,
                                first_unpublished,
                                next_display_deadline: Some(now + millis(8)),
                            };
                            assert_eq!(
                                decide(case, now, Duration::ZERO),
                                Publication::Now,
                                "a zero window publishes every turn: {case:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
