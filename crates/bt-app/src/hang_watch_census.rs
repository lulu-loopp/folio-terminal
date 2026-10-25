//! **What woke each turn, and what those turns cost** (T-IDLE-TURN-COST): the turn census
//! `BT_PERF_TRACE turns` prints, a reading of the turn accounting by wake source.
//!
//! The accounting already keeps every turn's wall time; what it did not keep was **what woke the
//! loop for it**. It knew the clock the loop parked until, and only when that clock came due or
//! was already past; a turn woken by an event — a shell's output, a watch's news, a redraw, a key —
//! had no source at all, and winit's own `StartCause` was dropped. This module keeps the one
//! missing fact per turn and, at the turn's close, adds the turn and its wall time to its source's
//! two counters.
//!
//! # A turn's source, in the vocabularies that already name wakes
//!
//! - **`Stale(<owner>)`** — the loop parked until an instant that was already due, so winit came
//!   straight back: the `WakeOwner` of that instant (`Stale(none)` with no owner). Judged first,
//!   because such a turn is the accounting's `stale` and its `StartCause` says only that the time
//!   was reached.
//! - **`Timed(<owner>)`** — `StartCause::ResumeTimeReached`: the `WakeOwner` of the deadline the
//!   loop parked until (`ResumeTimeReached` with no owner).
//! - **`Event(<variant>)`** — `StartCause::WaitCancelled`, and the turn's first handler was
//!   `user_event`: the `AppEvent` variant's own name.
//! - **`Window(<station>)`** — the same, and the first handler was `window_event`: the station
//!   `window_event_station` names the event by (`EventRedraw`, `EventKey`, the pointer kinds…).
//! - **`WaitCancelled`** — the wait was cancelled and no handler of the turn named itself (a
//!   message winit answered on its own).
//! - **`Poll`**, **`Init`** — winit's causes of the same names.
//! - **`Untold`** — a turn the platform delivered no `StartCause` for (the run's first; a platform
//!   that sends none) and no handler named.
//!
//! # What it costs
//!
//! On the window thread, per turn: two relaxed stores at the wake, one relaxed load per handler
//! (and one store, for the first), and two relaxed adds at the close. No clock read — the wall
//! time is the accounting's — no lock, and no allocation except **once per `AppEvent` variant per
//! run**, when a variant is first seen and its name is written down
//! ([`Census::app_event`]). The watchdog reads the counters on its own two-second round and writes
//! the line only when `BT_PERF_TRACE` is set; nothing here is on a clock of its own.

use std::mem::Discriminant;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, AtomicU16, AtomicU64, Ordering};

use super::{STATION_COUNT, Station};
use crate::AppEvent;
use crate::wake_owner::WakeOwner;

/// **How the platform said it woke the window thread**: winit's `StartCause`, by its own names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Cause {
    ResumeTimeReached = 1,
    WaitCancelled = 2,
    Poll = 3,
    Init = 4,
}

impl Cause {
    fn from_byte(byte: u8) -> Option<Self> {
        [
            Self::ResumeTimeReached,
            Self::WaitCancelled,
            Self::Poll,
            Self::Init,
        ]
        .into_iter()
        .find(|cause| *cause as u8 == byte)
    }
}

/// The owners, plus one for none.
const OWNERS: usize = WakeOwner::ALL.len() + 1;

/// How many distinct `AppEvent` variants the census can name in one run: more than the enum has.
/// A variant past them is counted as `unkept=` rather than under a name.
pub(super) const APP_EVENT_SLOTS: usize = 96;

/// Where each family of sources starts in the counters; the timed ones first, from zero.
const STALE: usize = OWNERS;
const WINDOW: usize = STALE + OWNERS;
const APP: usize = WINDOW + STATION_COUNT;
const WAIT_CANCELLED: usize = APP + APP_EVENT_SLOTS;
const POLL: usize = WAIT_CANCELLED + 1;
const INIT: usize = POLL + 1;
const UNTOLD: usize = INIT + 1;

/// Every source the census counts.
pub(super) const SOURCES: usize = UNTOLD + 1;

/// An owner's index among [`OWNERS`]: its discriminant, or the last for none.
fn owner_index(owner: Option<WakeOwner>) -> usize {
    owner.map_or(OWNERS - 1, |owner| owner as usize)
}

/// **The census's counters, and the turn in progress's source.** Every writer is the window
/// thread; every field is a relaxed atomic, for the accounting's reason.
#[derive(Debug)]
pub(super) struct Census {
    /// This turn's [`Cause`] as its byte, or zero before the platform told one.
    cause: AtomicU8,
    /// The owner of the deadline this turn's park was until, as the accounting packs an owner.
    owner: AtomicU8,
    /// The source the turn's first named handler gave, plus one, or zero.
    event: AtomicU16,
    /// The `AppEvent` variants seen this run, each with its name, in the order first seen.
    names: [OnceLock<(Discriminant<AppEvent>, String)>; APP_EVENT_SLOTS],
    /// Turns whose `AppEvent` found no slot left.
    unkept: AtomicU64,
    /// Per source, its turns and their wall time.
    turns: [AtomicU64; SOURCES],
    wall_ns: [AtomicU64; SOURCES],
}

impl Census {
    pub(super) fn new() -> Self {
        Self {
            cause: AtomicU8::new(0),
            owner: AtomicU8::new(0),
            event: AtomicU16::new(0),
            names: std::array::from_fn(|_| OnceLock::new()),
            unkept: AtomicU64::new(0),
            turns: std::array::from_fn(|_| AtomicU64::new(0)),
            wall_ns: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    /// **A turn opens**, owed the wake whose owner is `owner_byte` (the accounting's packing; zero
    /// when the park was no wait until an instant): no cause told yet, no handler named.
    pub(super) fn begin(&self, owner_byte: u8) {
        self.cause.store(0, Ordering::Relaxed);
        self.owner.store(owner_byte, Ordering::Relaxed);
        self.event.store(0, Ordering::Relaxed);
    }

    /// **The platform said why it woke the thread.**
    pub(super) fn started(&self, cause: Cause) {
        self.cause.store(cause as u8, Ordering::Relaxed);
    }

    /// **A handler of this turn named itself by `source`**: the first one names the turn.
    fn named(&self, source: usize) {
        if self.event.load(Ordering::Relaxed) == 0 {
            self.event.store(
                u16::try_from(source + 1).expect("the sources fit u16"),
                Ordering::Relaxed,
            );
        }
    }

    /// **`user_event` was handed `event`.** The first sight of a variant writes its name down —
    /// `AppEvent`'s `Debug` up to its first character that is not part of an identifier — which
    /// is this module's one allocation, once per variant per run.
    pub(super) fn app_event(&self, event: &AppEvent) {
        if self.event.load(Ordering::Relaxed) != 0 {
            return;
        }
        let kind = std::mem::discriminant(event);
        for (slot, name) in self.names.iter().enumerate() {
            // A slot another writer filled between the look and the set is looked at again.
            loop {
                match name.get() {
                    Some((seen, _)) if *seen == kind => return self.named(APP + slot),
                    Some(_) => break,
                    None => {
                        let debug = format!("{event:?}");
                        let end = debug
                            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                            .unwrap_or(debug.len());
                        let _ = name.set((kind, debug[..end].to_owned()));
                    }
                }
            }
        }
        self.unkept.fetch_add(1, Ordering::Relaxed);
    }

    /// **`window_event` was handed an event `window_event_station` names `station`.**
    pub(super) fn window_event(&self, station: Station) {
        self.named(WINDOW + station as usize);
    }

    /// **The turn closes after `wall_ns`**; `stale` when its park was no wait. Its source gets
    /// the turn and its wall time.
    pub(super) fn close(&self, wall_ns: u64, stale: bool) {
        let owner = super::accounting::owner_of(self.owner.load(Ordering::Relaxed));
        let named = usize::from(self.event.load(Ordering::Relaxed)).checked_sub(1);
        let cause = Cause::from_byte(self.cause.load(Ordering::Relaxed));
        let source = if stale {
            STALE + owner_index(owner)
        } else {
            match (cause, named) {
                (Some(Cause::ResumeTimeReached), _) => owner_index(owner),
                (Some(Cause::Poll), _) => POLL,
                (Some(Cause::Init), _) => INIT,
                (_, Some(named)) => named,
                (Some(Cause::WaitCancelled), None) => WAIT_CANCELLED,
                (None, None) => UNTOLD,
            }
        };
        self.turns[source].fetch_add(1, Ordering::Relaxed);
        self.wall_ns[source].fetch_add(wall_ns, Ordering::Relaxed);
    }

    /// **The counters as they stand**, for the watchdog.
    pub(super) fn reading(&self) -> Reading {
        Reading {
            turns: std::array::from_fn(|index| self.turns[index].load(Ordering::Relaxed)),
            wall_ns: std::array::from_fn(|index| self.wall_ns[index].load(Ordering::Relaxed)),
            unkept: self.unkept.load(Ordering::Relaxed),
        }
    }

    /// **The name a line prints for `source`.**
    fn name(&self, source: usize) -> String {
        let owner = |index: usize| {
            u8::try_from(index)
                .ok()
                .and_then(WakeOwner::from_byte)
                .map(|owner| format!("{owner:?}"))
        };
        match source {
            source if source < STALE => owner(source).map_or_else(
                || String::from("ResumeTimeReached"),
                |o| format!("Timed({o})"),
            ),
            source if source < WINDOW => format!(
                "Stale({})",
                owner(source - STALE).unwrap_or_else(|| String::from("none"))
            ),
            source if source < APP => format!(
                "Window({:?})",
                Station::from_byte(u8::try_from(source - WINDOW).unwrap_or(0))
            ),
            source if source < WAIT_CANCELLED => {
                format!(
                    "Event({})",
                    self.names[source - APP]
                        .get()
                        .map_or("?", |(_, name)| name.as_str())
                )
            }
            WAIT_CANCELLED => String::from("WaitCancelled"),
            POLL => String::from("Poll"),
            INIT => String::from("Init"),
            _ => String::from("Untold"),
        }
    }
}

/// **The census's counters, read at one moment.**
#[derive(Clone, Debug)]
pub(super) struct Reading {
    turns: [u64; SOURCES],
    wall_ns: [u64; SOURCES],
    unkept: u64,
}

impl Reading {
    fn zero() -> Self {
        Self {
            turns: [0; SOURCES],
            wall_ns: [0; SOURCES],
            unkept: 0,
        }
    }
}

/// **The `turns` line for the window from `before` to `after`**, `window_ms` long:
/// `BT_PERF_TRACE turns window_ms=… turns=… us_total=… by_source=<source>:<turns>/<us>,… unkept=…`,
/// the sources that took a turn in it, most turns first (`by_source=-` when none did).
pub(super) fn turns_line(
    census: &Census,
    before: &Reading,
    after: &Reading,
    window_ms: u64,
) -> String {
    let mut sources: Vec<(usize, u64, u64)> = (0..SOURCES)
        .map(|source| {
            (
                source,
                after.turns[source].saturating_sub(before.turns[source]),
                after.wall_ns[source].saturating_sub(before.wall_ns[source]),
            )
        })
        .filter(|(_, turns, _)| *turns > 0)
        .collect();
    sources.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let turns: u64 = sources.iter().map(|(_, turns, _)| turns).sum();
    let wall_ns: u64 = sources.iter().map(|(_, _, ns)| ns).sum();
    let by_source = sources
        .iter()
        .map(|(source, turns, ns)| format!("{}:{turns}/{}", census.name(*source), ns / 1_000))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "BT_PERF_TRACE turns window_ms={window_ms} turns={turns} us_total={} by_source={} unkept={}",
        wall_ns / 1_000,
        if by_source.is_empty() {
            "-"
        } else {
            &by_source
        },
        after.unkept.saturating_sub(before.unkept)
    )
}

/// **The `threads` line for one census of this process's threads** (`None` per unnamed thread):
/// `BT_PERF_TRACE threads total=… named=[<name>:<count>,…] unnamed=…`, most threads first. A
/// name's spaces, commas, colons and brackets print as `_`, so the line stays one list.
pub(super) fn threads_line(threads: &[Option<String>]) -> String {
    let mut named: Vec<(String, usize)> = Vec::new();
    for name in threads.iter().flatten() {
        let name: String = name
            .chars()
            .map(|c| {
                if c.is_whitespace() || matches!(c, ',' | ':' | '[' | ']') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        match named.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, count)) => *count += 1,
            None => named.push((name, 1)),
        }
    }
    named.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    format!(
        "BT_PERF_TRACE threads total={} named=[{}] unnamed={}",
        threads.len(),
        named
            .iter()
            .map(|(name, count)| format!("{name}:{count}"))
            .collect::<Vec<_>>()
            .join(","),
        threads.iter().filter(|name| name.is_none()).count()
    )
}

/// **The watchdog's side**: the reading the last `turns` line was taken from, and the last
/// `threads` line said. Owned by the watchdog thread; its time is the heartbeat's.
#[derive(Debug)]
pub(super) struct Round {
    last: Option<(u64, Reading)>,
    threads_said: Option<String>,
}

impl Round {
    pub(super) const fn new() -> Self {
        Self {
            last: None,
            threads_said: None,
        }
    }

    /// **One watchdog round at `now_ms`.** Untraced, nothing is read or written. Traced, the
    /// process's threads are counted (`threads`) and their line goes to `perf` when it differs
    /// from the last one said — the first round's always, and after that a pane opened or closed,
    /// a worker started or ended — and then the `turns` line for the window since the last round
    /// (since the heartbeat's origin, the first time).
    pub(super) fn tick(
        &mut self,
        now_ms: u64,
        trace: bool,
        census: &Census,
        threads: impl FnOnce() -> std::io::Result<Vec<Option<String>>>,
        mut perf: impl FnMut(String),
    ) {
        if !trace {
            return;
        }
        let counted = match threads() {
            Ok(threads) => threads_line(&threads),
            Err(error) => format!("BT_PERF_TRACE threads unavailable error={error}"),
        };
        if self.threads_said.as_deref() != Some(counted.as_str()) {
            perf(counted.clone());
            self.threads_said = Some(counted);
        }
        let after = census.reading();
        let (since_ms, before) = self.last.take().unwrap_or_else(|| (0, Reading::zero()));
        perf(turns_line(
            census,
            &before,
            &after,
            now_ms.saturating_sub(since_ms),
        ));
        self.last = Some((now_ms, after));
    }
}

#[cfg(test)]
mod tests {
    //! The census on a scripted turn sequence: every source each kind of turn is filed under, and
    //! the two lines' shapes.
    use super::{Cause, Census, Round, threads_line};
    use crate::AppEvent;
    use crate::hang_watch::Station;
    use crate::wake_owner::WakeOwner;

    const MS: u64 = 1_000_000;

    /// One turn: owed `owner`'s wake (packed as the accounting packs it), woken for `cause`,
    /// handed `events`, closed after `wall_ns`.
    fn turn(
        census: &Census,
        owner: Option<WakeOwner>,
        cause: Option<Cause>,
        events: &[Handed],
        wall_ns: u64,
        stale: bool,
    ) {
        census.begin(owner.map_or(0, |owner| owner as u8 + 1));
        if let Some(cause) = cause {
            census.started(cause);
        }
        for event in events {
            match event {
                Handed::App(event) => census.app_event(event),
                Handed::Window(station) => census.window_event(*station),
            }
        }
        census.close(wall_ns, stale);
    }

    enum Handed {
        App(AppEvent),
        Window(Station),
    }

    fn lines(census: &Census, window_ms: u64) -> Vec<String> {
        let mut said = Vec::new();
        Round::new().tick(
            window_ms,
            true,
            census,
            || Ok(vec![Some(String::from("folio-watchdog")), None]),
            |line| said.push(line),
        );
        said
    }

    /// RED (mutation: file a timed turn under its first event, or a stale one under its cause, or
    /// keep the second handler's name instead of the first's) — **each turn is filed under the
    /// source that woke it, and the line counts and times them**: three PtyOutput turns, two shell
    /// caret wakes, one stale session-save spin, one redraw, one untold, one Poll, one cancelled
    /// wait no handler named.
    #[test]
    fn each_turn_is_filed_under_what_woke_it() {
        let census = Census::new();
        let cancelled = Some(Cause::WaitCancelled);
        for _ in 0..3 {
            turn(
                &census,
                Some(WakeOwner::ShellCaret),
                cancelled,
                &[
                    Handed::App(AppEvent::PtyOutput),
                    Handed::Window(Station::EventRedraw),
                ],
                2 * MS,
                false,
            );
        }
        for _ in 0..2 {
            turn(
                &census,
                Some(WakeOwner::ShellCaret),
                Some(Cause::ResumeTimeReached),
                &[Handed::Window(Station::EventRedraw)],
                MS,
                false,
            );
        }
        turn(
            &census,
            Some(WakeOwner::SessionSave),
            Some(Cause::ResumeTimeReached),
            &[],
            MS / 2,
            true,
        );
        turn(
            &census,
            None,
            cancelled,
            &[Handed::Window(Station::EventRedraw)],
            3 * MS,
            false,
        );
        turn(&census, None, None, &[], MS, false);
        turn(&census, None, Some(Cause::Poll), &[], MS, false);
        turn(&census, None, cancelled, &[], MS, false);
        let said = lines(&census, 2_000);
        assert_eq!(
            said,
            [
                "BT_PERF_TRACE threads total=2 named=[folio-watchdog:1] unnamed=1",
                "BT_PERF_TRACE turns window_ms=2000 turns=10 us_total=14500 \
                 by_source=Event(PtyOutput):3/6000,Timed(ShellCaret):2/2000,\
                 Stale(SessionSave):1/500,Window(EventRedraw):1/3000,WaitCancelled:1/1000,\
                 Poll:1/1000,Untold:1/1000 unkept=0",
            ]
        );
    }

    /// RED (mutation: print the run's totals instead of the window's, or say the threads only
    /// once) — **a round prints only what happened since the last one, and the threads whenever
    /// they changed.**
    #[test]
    fn a_round_prints_the_window_since_the_last() {
        let census = Census::new();
        let mut round = Round::new();
        let mut said = Vec::new();
        let one = || Ok(vec![Some(String::from("main"))]);
        turn(
            &census,
            None,
            Some(Cause::WaitCancelled),
            &[Handed::App(AppEvent::GitChanged)],
            MS,
            false,
        );
        round.tick(2_000, true, &census, one, |line| said.push(line));
        turn(
            &census,
            None,
            Some(Cause::WaitCancelled),
            &[Handed::App(AppEvent::LayoutTablesReady)],
            2 * MS,
            false,
        );
        round.tick(4_100, true, &census, one, |line| said.push(line));
        round.tick(
            6_100,
            true,
            &census,
            || Ok(vec![Some(String::from("main")), None]),
            |line| said.push(line),
        );
        assert_eq!(
            said,
            [
                "BT_PERF_TRACE threads total=1 named=[main:1] unnamed=0",
                "BT_PERF_TRACE turns window_ms=2000 turns=1 us_total=1000 by_source=Event(GitChanged):1/1000 unkept=0",
                "BT_PERF_TRACE turns window_ms=2100 turns=1 us_total=2000 by_source=Event(LayoutTablesReady):1/2000 unkept=0",
                "BT_PERF_TRACE threads total=2 named=[main:1] unnamed=1",
                "BT_PERF_TRACE turns window_ms=2000 turns=0 us_total=0 by_source=- unkept=0",
            ]
        );
    }

    /// RED (mutation: read the counters on an untraced run) — **untraced, a round asks nothing
    /// and says nothing.**
    #[test]
    fn an_untraced_round_is_silent() {
        let census = Census::new();
        let mut said = Vec::new();
        Round::new().tick(
            2_000,
            false,
            &census,
            || panic!("the thread census is not taken untraced"),
            |line| said.push(line),
        );
        assert!(said.is_empty());
    }

    /// RED (mutation: count a name's threads once, or print a comma inside a name) — **the
    /// threads line groups by name and keeps the list one list.**
    #[test]
    fn the_threads_line_groups_by_name() {
        let threads = [
            Some(String::from("pty reader")),
            None,
            Some(String::from("pty reader")),
            Some(String::from("a,b")),
            None,
            None,
        ];
        assert_eq!(
            threads_line(&threads),
            "BT_PERF_TRACE threads total=6 named=[pty_reader:2,a_b:1] unnamed=3"
        );
    }
}
