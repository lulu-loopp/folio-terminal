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
//! ([`Census::app_event`]). Each station change adds the time spent in the station it leaves to
//! that station's counter ([`Census::station`]: one relaxed add, on the clock reading the change
//! already took). The watchdog reads the counters on its own two-second round and writes the
//! lines only when `BT_PERF_TRACE` is set; nothing here is on a clock of its own.

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
    /// **Per station, the window thread's time in it**, in nanoseconds, for the run
    /// (T-TWO-BARE-TURNS-PER-MARK-STEP): what the turns above were spent on, station by
    /// station. Charged by the heartbeat's one charge, on the clock reading that charge already
    /// took; the hold's own ledger counts whole milliseconds, and an empty turn is a third of one.
    station_ns: [AtomicU64; STATION_COUNT],
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
            station_ns: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    /// **The window thread spent `ns` in `station`**, just now: one relaxed add.
    pub(super) fn station(&self, station: Station, ns: u64) {
        self.station_ns[station.slot()].fetch_add(ns, Ordering::Relaxed);
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
            station_ns: std::array::from_fn(|slot| self.station_ns[slot].load(Ordering::Relaxed)),
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
    station_ns: [u64; STATION_COUNT],
    unkept: u64,
}

impl Reading {
    fn zero() -> Self {
        Self {
            turns: [0; SOURCES],
            wall_ns: [0; SOURCES],
            station_ns: [0; STATION_COUNT],
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

/// **The `stations` line for the window from `before` to `after`**, `window_ms` long
/// (T-TWO-BARE-TURNS-PER-MARK-STEP):
/// `BT_PERF_TRACE stations window_ms=… us_total=… by_station=<station>:<us>,…`, every station
/// the window thread spent a microsecond or more in, the most first. `us_total` is every
/// station's time, the ones under a microsecond included. It is the `turns` line's time, item by
/// item, and so the floor of an empty turn's: divided by that line's turns, a station's time is
/// what one turn spends in it. `None` for a window the thread spent no time in at all: there is
/// nothing to itemize, and the `turns` line beside it already says so.
pub(super) fn stations_line(before: &Reading, after: &Reading, window_ms: u64) -> Option<String> {
    let spent: Vec<(usize, u64)> = (0..STATION_COUNT)
        .map(|slot| {
            (
                slot,
                after.station_ns[slot].saturating_sub(before.station_ns[slot]),
            )
        })
        .collect();
    let total_ns: u64 = spent.iter().map(|(_, ns)| ns).sum();
    if total_ns == 0 {
        return None;
    }
    let mut listed: Vec<(usize, u64)> = spent
        .into_iter()
        .filter(|(_, ns)| *ns >= 1_000)
        .collect();
    listed.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let by_station = listed
        .iter()
        .map(|(slot, ns)| {
            format!(
                "{:?}:{}",
                Station::from_byte(u8::try_from(*slot).unwrap_or(0)),
                ns / 1_000
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Some(format!(
        "BT_PERF_TRACE stations window_ms={window_ms} us_total={} by_station={}",
        total_ns / 1_000,
        if by_station.is_empty() {
            "-"
        } else {
            &by_station
        }
    ))
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
    /// (since the heartbeat's origin, the first time), and its `stations` line when the window
    /// thread spent any time at all in that window.
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
        let window_ms = now_ms.saturating_sub(since_ms);
        perf(turns_line(census, &before, &after, window_ms));
        if let Some(stations) = stations_line(&before, &after, window_ms) {
            perf(stations);
        }
        self.last = Some((now_ms, after));
    }
}

#[cfg(test)]
mod tests {
    //! The census on a scripted turn sequence: every source each kind of turn is filed under, and
    //! the two lines' shapes.
    use super::{Cause, Census, Reading, Round, threads_line};
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

    /// RED (mutation: print the run's totals instead of the window's, drop the stations under a
    /// millisecond, or print the line for a window that spent nothing) — **the `stations` line
    /// itemizes the window's time by station, to the microsecond, most first**
    /// (T-TWO-BARE-TURNS-PER-MARK-STEP). The heartbeat's ledger counts whole milliseconds; an
    /// empty turn is a third of one, so its items are this line's or nobody's.
    #[test]
    fn the_stations_line_itemizes_the_window_to_the_microsecond() {
        let census = Census::new();
        let mut round = Round::new();
        let mut said = Vec::new();
        let one = || Ok(vec![Some(String::from("main"))]);
        round.tick(2_000, true, &census, one, |line| said.push(line));
        census.station(Station::Place, 40_000);
        census.station(Station::Drain, 250_500);
        census.station(Station::Place, 2_000);
        census.station(Station::Woken, 700);
        round.tick(4_000, true, &census, one, |line| said.push(line));
        census.station(Station::TurnPresent, 3_000_000);
        round.tick(6_000, true, &census, one, |line| said.push(line));
        assert_eq!(
            said,
            [
                "BT_PERF_TRACE threads total=1 named=[main:1] unnamed=0",
                "BT_PERF_TRACE turns window_ms=2000 turns=0 us_total=0 by_source=- unkept=0",
                "BT_PERF_TRACE turns window_ms=2000 turns=0 us_total=0 by_source=- unkept=0",
                "BT_PERF_TRACE stations window_ms=2000 us_total=293 by_station=Drain:250,Place:42",
                "BT_PERF_TRACE turns window_ms=2000 turns=0 us_total=0 by_source=- unkept=0",
                "BT_PERF_TRACE stations window_ms=2000 us_total=3000 by_station=TurnPresent:3000",
            ]
        );
    }

    /// RED (mutation: charge the census in whole milliseconds as the ledger is, or charge the
    /// parked stretch) — **the heartbeat's one charge feeds each station's time to the
    /// nanosecond, and the loop's park is nobody's** (T-TWO-BARE-TURNS-PER-MARK-STEP).
    #[test]
    fn the_heartbeats_charge_feeds_each_station_to_the_nanosecond() {
        use crate::hang_watch::{Heartbeat, Park, set_test_clock_ns};
        let heart = Heartbeat::on_test_clock();
        set_test_clock_ns(5_000);
        heart.woke();
        set_test_clock_ns(45_000);
        heart.at(Station::Place);
        set_test_clock_ns(295_500);
        heart.at(Station::Drain);
        set_test_clock_ns(300_000);
        heart.park(Park::Indefinite);
        // Parked for 8.7 ms: no station's.
        set_test_clock_ns(9_000_000);
        heart.woke();
        set_test_clock_ns(9_010_000);
        heart.park(Park::Indefinite);
        assert_eq!(
            super::stations_line(&Reading::zero(), &heart.accounts.census.reading(), 2_000)
                .as_deref(),
            Some(
                "BT_PERF_TRACE stations window_ms=2000 us_total=305 \
                 by_station=Place:250,Woken:50,Drain:4"
            )
        );
    }

    /// RED (mutation: take the thread census before the trace check) — **untraced, a round
    /// walks no threads and says nothing.**
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
