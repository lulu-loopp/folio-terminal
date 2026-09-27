//! **Every turn of the window thread is accounted, and every admitted wait is measured per
//! call** (0.4.6 ticket A3; `docs/plans/design/window-thread-budget-2026-09-25.md` §R-C, with
//! §C-7's acceptance notes).
//!
//! The slow-hold line says where a hold's milliseconds went once the hold has run past half a
//! second. What it cannot say is anything about the turns under that line — a turn that spent
//! 40 ms waiting, the budget's whole subject, left no trace — and its clock counts milliseconds,
//! so a 0.9 ms call charged nothing and two 3 ms calls read the same as one 6 ms call. This module
//! is the other half: a record of **every** turn, and of **every** admitted call, kept in
//! nanoseconds until a line is printed.
//!
//! # What is kept
//!
//! - **Per call**, in the meter's `leave`: the inclusive duration of the admitted call (the two
//!   instants `admitted` read around its work), folded into the door's histogram — a count, a
//!   sum, a maximum and [`BUCKETS`] power-of-two buckets, in per-run atomics, one per registry
//!   line (`bt_platform::admission::doors::ALL`).
//! - **Per turn**, at `park` and before the slow-hold threshold is looked at, whether or not any
//!   queue takes a line: the turn's wall time, the **union** of its admitted calls, the time that
//!   was not an admitted call (**unexplained**, §C-7: a named station with no admitted call under
//!   it is unexplained too), and the scheduling delay — how late the turn began after the wake the
//!   loop was owed. Each into its own histogram.
//!
//! # The union, exactly and without a buffer
//!
//! Admitted calls on one thread nest: an inner call's interval lies inside its outer call's,
//! because `admitted` is synchronous and holds its own cookie on its own stack. So the union of a
//! turn's intervals is the sum of its **outermost** calls, and nothing else needs to be kept: the
//! meter's `enter` saves the nesting depth in the cookie (bits 40–47), `leave` puts it back, and a
//! call that left at depth zero adds its interval, clipped to the turn's start. Nested calls
//! neither hide latency nor count twice, whatever their number — there is no interval buffer and
//! so no capacity past which the union would degrade.
//!
//! # The four triggers, and how a line leaves the window thread
//!
//! A turn offers a detail line when its wall time runs past its frame (`T − t₀`: the shortest
//! frame interval among the visible windows that took a turn, else [`TURN_BUDGET`]), when the
//! union of its waits runs past [`WAIT_BUDGET`], when a call ran past its bound ([`WAIT_ALLOWANCE`]
//! — the registry rules no larger bound for any row; the line names the turn's longest), and when
//! its unexplained time runs past [`WAIT_BUDGET`]. Each trigger writes at most one line per
//! [`COALESCE_WINDOW`]: a turn inside it is counted, and the trigger's next line carries the count
//! (`suppressed=`), while every turn still goes into the histograms. The lines are plain data
//! pushed into a ring of
//! [`BUDGET_LINES_KEPT`] that the window thread only ever `try_lock`s: a push that finds it held
//! or full is counted as lost, and the next line printed and the exit summary carry the count.
//! The watchdog formats and writes them; the exit summary is written from the atomics, so it is
//! whole however many lines were lost.
//!
//! # What it costs
//!
//! No clock read of its own. The turn's two ends are the heartbeat's own reads (`woke`, `park`),
//! now kept in nanoseconds; a call's two ends are the instants `admitted` already reads, and the
//! meter's `leave` restores the station on the call's end instant rather than reading the clock
//! again — one read fewer per admitted call than before this module. A parked loop runs none of
//! it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bt_platform::admission::{DoorKey, doors};

use super::Station;

/// **A turn's frame when no visible window names one**: `T − t₀` with no `FrameClock` to read,
/// which is `pace::DEFAULT_FRAME_INTERVAL` (budget note §1.2, §R-B).
pub(super) const TURN_BUDGET: Duration = crate::pace::DEFAULT_FRAME_INTERVAL;

/// **The waits of one turn, together** — half a turn, the argument `DRAIN_TURN_BUDGET` was sized
/// by (budget note §1.2). Also the target unexplained time is measured against (§R-C 2).
pub(super) const WAIT_BUDGET: Duration = Duration::from_millis(8);

/// **One call to one admitted wait** — a quarter of a turn (budget note §1.2). The bound every
/// registry row is held to: a larger one is admitted only by a ruling (§1.4), and the registry
/// records none.
pub(super) const WAIT_ALLOWANCE: Duration = Duration::from_millis(4);

/// How many detail lines may wait for the watchdog. It drains every `WATCH_INTERVAL` (two
/// seconds), so this bounds what the log can be given to eight lines a second however badly the
/// turns are going; what does not fit is counted.
pub(super) const BUDGET_LINES_KEPT: usize = 16;

/// **At most one line per trigger in this long** (coordinator's ruling, 2026-09-27): a turn that
/// would write a trigger's line within it of that trigger's last line is counted instead, and the
/// next line of that trigger says how many (`suppressed=`). Measured on the heartbeat's clock.
pub(super) const COALESCE_WINDOW: Duration = Duration::from_secs(1);

/// The four triggers, one coalescing window each.
const TRIGGERS: usize = 4;

/// A histogram's buckets: under 1 µs, then one per power of two of microseconds, the last open
/// above (4.19 s and up).
pub(super) const BUCKETS: usize = 24;

/// One registry line per door type.
const DOORS: usize = doors::ALL.len();

/// A frame interval not yet told this turn.
const NO_FRAME: u64 = u64::MAX;

/// Where a kept call's door index starts; the nanoseconds are below it.
const DOOR_SHIFT: u32 = 48;

/// `duration` in the nanoseconds everything here counts in.
pub(super) const fn nanos(duration: Duration) -> u64 {
    duration.as_secs() * 1_000_000_000 + duration.subsec_nanos() as u64
}

/// The bucket `ns` falls in: 0 below a microsecond, `k` for `[2^(k-1), 2^k)` µs.
fn bucket(ns: u64) -> usize {
    let micros = ns / 1_000;
    if micros == 0 {
        0
    } else {
        ((u64::BITS - micros.leading_zeros()) as usize).min(BUCKETS - 1)
    }
}

/// The lower edge of bucket `index`, as the summary prints it.
fn bucket_label(index: usize) -> String {
    match index {
        0 => String::from("<1us"),
        index if index == BUCKETS - 1 => format!("{}us+", 1_u64 << (index - 1)),
        index => format!("{}us", 1_u64 << (index - 1)),
    }
}

/// A duration as the whole microseconds a line prints; everything before printing is nanoseconds.
fn micros(ns: u64) -> u64 {
    ns / 1_000
}

/// **Count, sum, maximum and buckets of one quantity, for the run**, in relaxed atomics: the
/// writer is the window thread and the reader is the exit summary, and neither may wait.
#[derive(Debug)]
struct Histogram {
    count: AtomicU64,
    sum_ns: AtomicU64,
    max_ns: AtomicU64,
    buckets: [AtomicU64; BUCKETS],
}

impl Histogram {
    fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            sum_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn record(&self, ns: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
        self.max_ns.fetch_max(ns, Ordering::Relaxed);
        self.buckets[bucket(ns)].fetch_add(1, Ordering::Relaxed);
    }

    fn tally(&self) -> Tally {
        Tally {
            count: self.count.load(Ordering::Relaxed),
            sum_ns: self.sum_ns.load(Ordering::Relaxed),
            max_ns: self.max_ns.load(Ordering::Relaxed),
            buckets: std::array::from_fn(|index| self.buckets[index].load(Ordering::Relaxed)),
        }
    }
}

/// **One histogram, read.**
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tally {
    pub count: u64,
    pub sum_ns: u64,
    pub max_ns: u64,
    pub buckets: [u64; BUCKETS],
}

impl Tally {
    /// `count=… max_us=… sum_us=… hist=<bucket>:<count>,…`, the non-empty buckets only.
    fn fields(&self) -> String {
        let hist = self
            .buckets
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(index, count)| format!("{}:{count}", bucket_label(index)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "count={} max_us={} sum_us={} hist={}",
            self.count,
            micros(self.max_ns),
            micros(self.sum_ns),
            if hist.is_empty() { "-" } else { &hist }
        )
    }
}

/// **What one detail line reports.** Plain data, formatted on the watchdog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Budget {
    /// The turn's wall time ran past its frame. The scheduling delay is beside it and not in it.
    Wall {
        wall_ns: u64,
        frame_ns: u64,
        waits_ns: u64,
        unexplained_ns: u64,
        delay_ns: Option<u64>,
    },
    /// The union of the turn's admitted calls ran past [`WAIT_BUDGET`].
    Waits { waits_ns: u64, calls: u64 },
    /// An admitted call ran past its row's bound: the turn's longest such call. `more` counts the
    /// turn's other calls past their bound.
    Call { door: DoorKey, ns: u64, more: u64 },
    /// The turn's time outside every admitted call ran past [`WAIT_BUDGET`]: not a wait, and
    /// never reported as one.
    Unexplained { unexplained_ns: u64, wall_ns: u64 },
}

impl Budget {
    /// Which of the four triggers this is: the index of its coalescing window.
    fn trigger(&self) -> usize {
        match self {
            Self::Wall { .. } => 0,
            Self::Waits { .. } => 1,
            Self::Call { .. } => 2,
            Self::Unexplained { .. } => 3,
        }
    }
}

/// **One detail line**, as the window thread queued it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetLine {
    /// The turn counter as it stood at `park`.
    pub turn: u64,
    pub budget: Budget,
    /// The turns since this trigger's last line that crossed it inside [`COALESCE_WINDOW`] and
    /// wrote nothing.
    pub suppressed: u64,
}

impl BudgetLine {
    /// The line the diagnostics log gets. `refused` is the process's admission refusals and
    /// `lost` the lines lost since the last line printed; both are on every line, so a line never
    /// reads as clean beside drops (budget note §R-C 4, §C-5).
    #[must_use]
    pub fn line(&self, refused: u64, lost: u64) -> String {
        let body = match self.budget {
            Budget::Wall {
                wall_ns,
                frame_ns,
                waits_ns,
                unexplained_ns,
                delay_ns,
            } => format!(
                "wall us={} frame_us={} waits_us={} unexplained_us={} delay_us={}",
                micros(wall_ns),
                micros(frame_ns),
                micros(waits_ns),
                micros(unexplained_ns),
                delay_ns.map_or_else(|| String::from("none"), |ns| micros(ns).to_string())
            ),
            Budget::Waits { waits_ns, calls } => format!(
                "waits us={} budget_us={} calls={calls}",
                micros(waits_ns),
                micros(nanos(WAIT_BUDGET))
            ),
            Budget::Call { door, ns, more } => {
                let mut body = format!(
                    "call us={} bound_us={} door={} row={} station={:?}",
                    micros(ns),
                    micros(nanos(WAIT_ALLOWANCE)),
                    door.name(),
                    door.row().label(),
                    Station::from_byte(door.station())
                );
                if more > 0 {
                    body.push_str(&format!(" more={more}"));
                }
                body
            }
            Budget::Unexplained {
                unexplained_ns,
                wall_ns,
            } => format!(
                "unexplained us={} target_us={} wall_us={}",
                micros(unexplained_ns),
                micros(nanos(WAIT_BUDGET)),
                micros(wall_ns)
            ),
        };
        format!(
            "Folio budget: turn={} {body} suppressed={} refused={refused} lost={lost}",
            self.turn, self.suppressed
        )
    }
}

/// **The lines the watchdog writes for `lines`**, in order. Every one carries `refused`; the first
/// carries the losses since the last line printed — `lost` less `printed`, which it then brings up
/// to `lost` — and the rest carry none, so each loss is said once, on the next line after it.
pub(super) fn said(
    lines: Vec<BudgetLine>,
    lost: u64,
    printed: &mut u64,
    refused: u64,
) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            let since = lost.saturating_sub(*printed);
            *printed = lost;
            line.line(refused, since)
        })
        .collect()
}

/// **The run's accounts, and the turn in progress.** Owned by the heartbeat; every writer is the
/// window thread and every field is a relaxed atomic, for the heartbeat's own reason — the writer
/// is the thread this exists to diagnose.
#[derive(Debug)]
pub(super) struct Accounts {
    /// The wake the loop is about to park until, in heartbeat nanoseconds plus one, or zero (see
    /// [`Self::owe`]).
    due_ns: AtomicU64,
    /// That wake, carried from `park` to the turn that follows it.
    owed_ns: AtomicU64,
    /// This turn's scheduling delay plus one, or zero when no owed wake had come due.
    delay_ns: AtomicU64,
    /// The shortest frame interval a visible window told this turn, or [`NO_FRAME`].
    frame_ns: AtomicU64,
    /// Admitted calls open on the window thread right now.
    depth: AtomicU64,
    /// This turn's union of admitted calls.
    waits_ns: AtomicU64,
    /// This turn's admitted calls.
    calls: AtomicU64,
    /// This turn's longest call past its bound: the door's index above [`DOOR_SHIFT`],
    /// nanoseconds below.
    longest_over: AtomicU64,
    /// How many calls past their bound this turn had.
    over_count: AtomicU64,
    /// Per trigger, when its last line was queued, plus one; zero before its first.
    last_line_ns: [AtomicU64; TRIGGERS],
    /// Per trigger, the turns coalesced since its last line.
    suppressed: [AtomicU64; TRIGGERS],
    wall: Histogram,
    waits: Histogram,
    unexplained: Histogram,
    delay: Histogram,
    /// Per registry line, in `doors::ALL`'s order.
    doors: [Histogram; DOORS],
    /// Detail lines waiting for the watchdog. Only ever `try_lock`ed by the window thread.
    lines: Mutex<Vec<BudgetLine>>,
    /// Pushes that found [`Self::lines`] held or full, for the run.
    lost: AtomicU64,
}

impl Accounts {
    pub(super) fn new() -> Self {
        Self {
            due_ns: AtomicU64::new(0),
            owed_ns: AtomicU64::new(0),
            delay_ns: AtomicU64::new(0),
            frame_ns: AtomicU64::new(NO_FRAME),
            depth: AtomicU64::new(0),
            waits_ns: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            longest_over: AtomicU64::new(0),
            over_count: AtomicU64::new(0),
            last_line_ns: std::array::from_fn(|_| AtomicU64::new(0)),
            suppressed: std::array::from_fn(|_| AtomicU64::new(0)),
            wall: Histogram::new(),
            waits: Histogram::new(),
            unexplained: Histogram::new(),
            delay: Histogram::new(),
            doors: std::array::from_fn(|_| Histogram::new()),
            lines: Mutex::new(Vec::with_capacity(BUDGET_LINES_KEPT)),
            lost: AtomicU64::new(0),
        }
    }

    /// **The loop is about to park until `deadline_ns`**: the wake it will be owed.
    pub(super) fn owe(&self, deadline_ns: u64) {
        self.due_ns
            .store(deadline_ns.saturating_add(1), Ordering::Relaxed);
    }

    /// **The loop parked**: owed the wake [`Self::owe`] named when it parked until one, and owed
    /// nothing otherwise.
    pub(super) fn parked(&self, until: bool) {
        let due = self.due_ns.swap(0, Ordering::Relaxed);
        self.owed_ns
            .store(if until { due } else { 0 }, Ordering::Relaxed);
    }

    /// **A turn opens at `now_ns`**: no frame told yet, no admitted call open, and a wake that
    /// had come due by now is its scheduling delay. Its calls start empty because the last turn's
    /// close took them, and a call outside every turn adds none ([`Self::call`]).
    pub(super) fn begin(&self, now_ns: u64) {
        let owed = self.owed_ns.swap(0, Ordering::Relaxed);
        let delay = match owed.checked_sub(1) {
            Some(due) if now_ns >= due => (now_ns - due).saturating_add(1),
            _ => 0,
        };
        self.delay_ns.store(delay, Ordering::Relaxed);
        self.frame_ns.store(NO_FRAME, Ordering::Relaxed);
        self.depth.store(0, Ordering::Relaxed);
    }

    /// **A visible window's frame is `interval_ns` long.** The turn's frame is the shortest.
    pub(super) fn frame(&self, interval_ns: u64) {
        self.frame_ns.fetch_min(interval_ns, Ordering::Relaxed);
    }

    /// **An admitted call begins**: answers the depth it found, for its cookie.
    pub(super) fn enter(&self) -> u64 {
        self.depth.fetch_add(1, Ordering::Relaxed)
    }

    /// **An admitted call ended**: the depth it found is the depth again, whatever a call under it
    /// that unwound left behind.
    pub(super) fn restore_depth(&self, depth: u64) {
        self.depth.store(depth, Ordering::Relaxed);
    }

    /// **One admitted call of `door`, from `start_ns` to `end_ns`.** Its inclusive duration goes
    /// to the door's histogram whenever it ran. Inside a turn (`turn_began_ns`) it is one of the
    /// turn's calls; an outermost call adds its interval, clipped to the turn's start, to the
    /// union; and a call past its bound is counted, the longest kept for the turn's line.
    pub(super) fn call(
        &self,
        door: DoorKey,
        start_ns: u64,
        end_ns: u64,
        outermost: bool,
        turn_began_ns: Option<u64>,
    ) {
        let ns = end_ns.saturating_sub(start_ns);
        let index = doors::ALL.iter().position(|key| *key == door);
        if let Some(index) = index {
            self.doors[index].record(ns);
        }
        let Some(began) = turn_began_ns else {
            return;
        };
        self.calls.fetch_add(1, Ordering::Relaxed);
        if outermost {
            self.waits_ns.fetch_add(
                end_ns.saturating_sub(start_ns.max(began)),
                Ordering::Relaxed,
            );
        }
        if ns > nanos(WAIT_ALLOWANCE)
            && let Some(index) = index
        {
            let packed = ((index as u64) << DOOR_SHIFT) | ns.min((1 << DOOR_SHIFT) - 1);
            let longest = self.longest_over.load(Ordering::Relaxed);
            if self.over_count.fetch_add(1, Ordering::Relaxed) == 0
                || ns > (longest & ((1 << DOOR_SHIFT) - 1))
            {
                self.longest_over.store(packed, Ordering::Relaxed);
            }
        }
    }

    /// **The turn that began at `began_ns` parks at `now_ns`**: its calls are taken, its record
    /// goes into the run's histograms, and each trigger it crossed offers a line
    /// ([`Self::offer`]).
    pub(super) fn close(&self, turn: u64, began_ns: u64, now_ns: u64) {
        let wall_ns = now_ns.saturating_sub(began_ns);
        let waits_ns = self.waits_ns.swap(0, Ordering::Relaxed).min(wall_ns);
        let calls = self.calls.swap(0, Ordering::Relaxed);
        let over = self.over_count.swap(0, Ordering::Relaxed);
        let unexplained_ns = wall_ns - waits_ns;
        let delay_ns = self.delay_ns.load(Ordering::Relaxed).checked_sub(1);
        let frame_ns = match self.frame_ns.load(Ordering::Relaxed) {
            NO_FRAME => nanos(TURN_BUDGET),
            frame => frame,
        };
        self.wall.record(wall_ns);
        self.waits.record(waits_ns);
        self.unexplained.record(unexplained_ns);
        if let Some(delay) = delay_ns {
            self.delay.record(delay);
        }
        if wall_ns > frame_ns {
            self.offer(
                turn,
                Budget::Wall {
                    wall_ns,
                    frame_ns,
                    waits_ns,
                    unexplained_ns,
                    delay_ns,
                },
                now_ns,
            );
        }
        if waits_ns > nanos(WAIT_BUDGET) {
            self.offer(turn, Budget::Waits { waits_ns, calls }, now_ns);
        }
        if over > 0 {
            let packed = self.longest_over.load(Ordering::Relaxed);
            if let Some(door) = usize::try_from(packed >> DOOR_SHIFT)
                .ok()
                .and_then(|index| doors::ALL.get(index))
            {
                self.offer(
                    turn,
                    Budget::Call {
                        door: *door,
                        ns: packed & ((1 << DOOR_SHIFT) - 1),
                        more: over - 1,
                    },
                    now_ns,
                );
            }
        }
        if unexplained_ns > nanos(WAIT_BUDGET) {
            self.offer(
                turn,
                Budget::Unexplained {
                    unexplained_ns,
                    wall_ns,
                },
                now_ns,
            );
        }
    }

    /// **Offer one trigger's line at `now_ns`.** Inside [`COALESCE_WINDOW`] of that trigger's last
    /// line it is counted as suppressed and not queued. Otherwise it is queued without waiting,
    /// carrying the count, which starts again; a ring held by the watchdog, or full, loses it and
    /// counts the loss, and the suppressed count waits for the next line that is queued.
    fn offer(&self, turn: u64, budget: Budget, now_ns: u64) {
        let trigger = budget.trigger();
        if let Some(last) = self.last_line_ns[trigger]
            .load(Ordering::Relaxed)
            .checked_sub(1)
            && now_ns.saturating_sub(last) < nanos(COALESCE_WINDOW)
        {
            self.suppressed[trigger].fetch_add(1, Ordering::Relaxed);
            return;
        }
        let suppressed = self.suppressed[trigger].load(Ordering::Relaxed);
        if let Ok(mut lines) = self.lines.try_lock()
            && lines.len() < BUDGET_LINES_KEPT
        {
            lines.push(BudgetLine {
                turn,
                budget,
                suppressed,
            });
            self.suppressed[trigger].store(0, Ordering::Relaxed);
            self.last_line_ns[trigger].store(now_ns.saturating_add(1), Ordering::Relaxed);
        } else {
            self.lost.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// **Every line queued since the last ask, and the run's losses so far.** Called from the
    /// watchdog; answers nothing rather than waiting when the window thread has the ring, and
    /// allocates nothing when there is nothing to take.
    pub(super) fn take_lines(&self) -> (Vec<BudgetLine>, u64) {
        let lines = match self.lines.try_lock() {
            Ok(mut lines) if !lines.is_empty() => lines.drain(..).collect(),
            _ => Vec::new(),
        };
        (lines, self.lost.load(Ordering::Relaxed))
    }

    /// **The exit summary, from the atomics alone**: the four turn quantities, every door that
    /// was called, and the losses and refusals. Whole however many lines were lost.
    pub(super) fn summary(&self, refused: u64) -> Vec<String> {
        let mut lines = Vec::new();
        for (name, histogram) in [
            ("wall", &self.wall),
            ("waits", &self.waits),
            ("unexplained", &self.unexplained),
            ("delay", &self.delay),
        ] {
            lines.push(format!(
                "Folio budget summary: {name} {}",
                histogram.tally().fields()
            ));
        }
        for (door, histogram) in doors::ALL.iter().zip(&self.doors) {
            let tally = histogram.tally();
            if tally.count > 0 {
                lines.push(format!(
                    "Folio budget summary: door={} row={} {}",
                    door.name(),
                    door.row().label(),
                    tally.fields()
                ));
            }
        }
        lines.push(format!(
            "Folio budget summary: lost={} refused={refused}",
            self.lost.load(Ordering::Relaxed)
        ));
        lines
    }

    /// One door's calls, read.
    #[cfg(test)]
    pub(super) fn door(&self, door: DoorKey) -> Tally {
        doors::ALL
            .iter()
            .position(|key| *key == door)
            .map(|index| self.doors[index].tally())
            .unwrap_or_default()
    }

    /// The turns' wall, waits, unexplained and delay, read.
    #[cfg(test)]
    pub(super) fn turns(&self) -> [Tally; 4] {
        [
            self.wall.tally(),
            self.waits.tally(),
            self.unexplained.tally(),
            self.delay.tally(),
        ]
    }

    /// The ring itself, for a case that holds it as the watchdog would.
    #[cfg(test)]
    pub(super) fn ring(&self) -> &Mutex<Vec<BudgetLine>> {
        &self.lines
    }
}

#[cfg(test)]
mod tests {
    //! A3's cases, on the injected clock: the heartbeat's `_at` verbs and the meter's two halves
    //! are handed nanoseconds, so every number below is exact (budget note §R-C's table, §C-7).

    use std::time::{Duration, Instant};

    use bt_platform::admission::{Door, DoorKey, doors};

    use super::super::{HangWatch, Heartbeat, Park, Station, Verdict};
    use super::{BUDGET_LINES_KEPT, Budget, BudgetLine, COALESCE_WINDOW, nanos, said};

    const US: u64 = 1_000;
    const MS: u64 = 1_000_000;

    fn heart() -> Heartbeat {
        Heartbeat::sampling(|| None)
    }

    fn key<D: Door>() -> DoorKey {
        D::KEY
    }

    /// One admitted call of `door` from `start_ns` to `end_ns`, through the meter's two halves as
    /// `admitted` calls them.
    fn call(heart: &Heartbeat, door: DoorKey, start_ns: u64, end_ns: u64) {
        let cookie = heart.admitted_enter_at(Station::from_byte(door.station()), start_ns / MS);
        assert!(heart.admitted_leave_between(door, cookie, start_ns, end_ns));
    }

    fn lines(heart: &Heartbeat) -> Vec<Budget> {
        heart
            .accounts
            .take_lines()
            .0
            .into_iter()
            .map(|line| line.budget)
            .collect()
    }

    /// RED (A3) — **one call of 4,001 µs prints one bound line, and nothing else.**
    ///
    /// The bound is the row's ([`super::WAIT_ALLOWANCE`], 4,000 µs: the registry rules no larger
    /// one), and "past" is strict, so a call of exactly 4,000 µs prints nothing. The turn's other
    /// three triggers stay quiet: its waits are under the budget, its wall time under its frame.
    ///
    /// MUTATION: hold a call to `WAIT_BUDGET` instead of `WAIT_ALLOWANCE` in `Accounts::call` and
    /// no line is printed.
    #[test]
    fn one_call_past_its_bound_prints_one_bound_line() {
        let heart = heart();
        let resize = key::<doors::PtyResize>();
        heart.woke_at_ns(0);
        call(&heart, resize, MS, MS + 4_001 * US);
        heart.park_at_ns(Park::Indefinite, 6 * MS);
        let (taken, lost) = heart.accounts.take_lines();
        assert_eq!(lost, 0);
        let [line] = taken.as_slice() else {
            panic!("one line: {taken:?}")
        };
        assert_eq!(
            line.budget,
            Budget::Call {
                door: resize,
                ns: 4_001 * US,
                more: 0
            }
        );
        assert_eq!(
            line.line(0, 0),
            "Folio budget: turn=0 call us=4001 bound_us=4000 door=PtyResize row=12 \
             station=PtyResize suppressed=0 refused=0 lost=0"
        );
        // Exactly at the bound is not past it (past the coalescing window, so a line would show).
        heart.woke_at_ns(2_000 * MS);
        call(&heart, resize, 2_001 * MS, 2_001 * MS + 4_000 * US);
        heart.park_at_ns(Park::Indefinite, 2_006 * MS);
        assert_eq!(lines(&heart), []);
    }

    /// RED (A3) — **two 3 ms calls in one turn print no per-call line, and the turn's waits are
    /// their union, 6 ms.**
    ///
    /// Before A3 the ledger charged exclusive milliseconds, so two 3 ms calls and one 6 ms call
    /// left the same record, and nothing could hold a call to its own bound.
    ///
    /// MUTATION: `store` instead of `fetch_add` the outermost call's interval into `waits_ns` and
    /// the union is 3 ms.
    #[test]
    fn two_three_ms_calls_print_no_per_call_line_and_a_six_ms_union() {
        let heart = heart();
        let resize = key::<doors::PtyResize>();
        heart.woke_at_ns(0);
        call(&heart, resize, MS, 4 * MS);
        call(&heart, key::<doors::TitleFlush>(), 5 * MS, 8 * MS);
        heart.park_at_ns(Park::Indefinite, 9 * MS);
        assert_eq!(lines(&heart), []);
        let [wall, waits, unexplained, _] = heart.accounts.turns();
        assert_eq!((wall.count, wall.sum_ns), (1, 9 * MS));
        assert_eq!((waits.count, waits.sum_ns), (1, 6 * MS));
        assert_eq!(unexplained.sum_ns, 3 * MS);
        assert_eq!(heart.accounts.door(resize).sum_ns, 3 * MS);
    }

    /// RED (A3, §C-7) — **5,000 calls of 0.9 µs sum to 4.5 ms exactly, not to zero.**
    ///
    /// Durations are nanoseconds until a line is printed: a call truncated to whole microseconds
    /// (or to the ledger's milliseconds) before it was summed would add nothing, 5,000 times.
    ///
    /// MUTATION: record `ns / 1_000 * 1_000` in `Accounts::call` and the sum is zero.
    #[test]
    fn five_thousand_sub_microsecond_calls_sum_exactly() {
        let heart = heart();
        let resize = key::<doors::PtyResize>();
        heart.woke_at_ns(0);
        for index in 0..5_000 {
            let start = MS + index * US;
            call(&heart, resize, start, start + 900);
        }
        heart.park_at_ns(Park::Indefinite, 7 * MS);
        let tally = heart.accounts.door(resize);
        assert_eq!(
            (tally.count, tally.sum_ns, tally.max_ns),
            (5_000, 4_500_000, 900)
        );
        assert_eq!(tally.buckets[0], 5_000, "every call is under a microsecond");
        assert_eq!(
            heart.accounts.turns()[1].sum_ns,
            4_500_000,
            "the turn's union"
        );
        assert!(
            heart.accounts.summary(0).contains(&String::from(
                "Folio budget summary: door=PtyResize row=12 count=5000 max_us=0 \
                     sum_us=4500 hist=<1us:5000"
            )),
            "{:#?}",
            heart.accounts.summary(0)
        );
    }

    /// RED (A3) — **9 ms of waits in one turn print one wait-union line.**
    ///
    /// Three calls of 3 ms, each under its bound, together past [`super::WAIT_BUDGET`]: the
    /// turn's line and no call's.
    ///
    /// MUTATION: compare the union against `TURN_BUDGET` instead of `WAIT_BUDGET` in
    /// `Accounts::close` and no line is printed.
    #[test]
    fn nine_ms_of_waits_print_a_wait_union_line() {
        let heart = heart();
        let resize = key::<doors::PtyResize>();
        heart.woke_at_ns(0);
        for start in [MS, 4 * MS, 7 * MS] {
            call(&heart, resize, start, start + 3 * MS);
        }
        heart.park_at_ns(Park::Indefinite, 11 * MS);
        let (taken, _) = heart.accounts.take_lines();
        let [line] = taken.as_slice() else {
            panic!("one line: {taken:?}")
        };
        assert_eq!(
            line.budget,
            Budget::Waits {
                waits_ns: 9 * MS,
                calls: 3
            }
        );
        assert_eq!(
            line.line(2, 0),
            "Folio budget: turn=0 waits us=9000 budget_us=8000 calls=3 suppressed=0 refused=2 \
             lost=0"
        );
    }

    /// RED (A3, §C-7) — **40 ms that no admitted call explains is unexplained, never a wait —
    /// outside every station, and inside a named station with no admitted call under it.**
    ///
    /// Unexplained means "not an admitted wait": a `Work` or `Scope` station names where the
    /// time went, and still does not make it a wait. Both runs print an unexplained line (and a
    /// wall line, 40 ms being past the frame) and neither prints a wait line or counts a wait.
    ///
    /// MUTATION: push the unexplained time as `Budget::Waits` in `Accounts::close` and both runs
    /// go red.
    #[test]
    fn forty_ms_unclassified_is_unexplained_not_a_wait() {
        for named in [false, true] {
            let heart = heart();
            heart.woke_at_ns(0);
            if named {
                let parent = heart.enter_at(Station::Drain, 0, 0);
                let super::super::Location::Resume {
                    station,
                    node,
                    scope,
                } = parent
                else {
                    unreachable!("`enter_at` answers a resume")
                };
                heart.resume_at(station, node, scope, 40);
            }
            heart.park_at_ns(Park::Indefinite, 40 * MS);
            let taken = lines(&heart);
            assert!(
                taken.contains(&Budget::Unexplained {
                    unexplained_ns: 40 * MS,
                    wall_ns: 40 * MS
                }),
                "named {named}: {taken:?}"
            );
            assert!(
                !taken
                    .iter()
                    .any(|line| matches!(line, Budget::Waits { .. } | Budget::Call { .. })),
                "named {named}: nothing here is a wait: {taken:?}"
            );
            let [_, waits, unexplained, _] = heart.accounts.turns();
            assert_eq!(waits.sum_ns, 0, "named {named}");
            assert_eq!(unexplained.sum_ns, 40 * MS, "named {named}");
        }
    }

    /// RED (A3) — **nested admitted calls count their union once.**
    ///
    /// A call inside another lies inside its interval, so counting both would charge the inner
    /// one twice; counting only the inner one would hide the outer one's own time. The union is
    /// the outer call's 6 ms, and each call still has its own inclusive record. A call under it
    /// that unwound (entered, never left) does not make the next call look nested.
    ///
    /// MUTATION: treat every call as outermost in `admitted_leave_between` and the union is 9 ms.
    #[test]
    fn nested_admitted_calls_count_the_union_once() {
        let heart = heart();
        let present = key::<doors::PresentFrame>();
        let commit = key::<doors::CompositorCommit>();
        heart.woke_at_ns(0);
        let outer = heart.admitted_enter_at(Station::RenderCompose, 1);
        let inner = heart.admitted_enter_at(Station::CompositorCommit, 2);
        assert!(heart.admitted_leave_between(commit, inner, 2 * MS, 5 * MS));
        // A call under the outer one that never came back.
        let _unwound = heart.admitted_enter_at(Station::CompositorCommit, 5);
        assert!(heart.admitted_leave_between(present, outer, MS, 7 * MS));
        // After the outer call, the next one is outermost again.
        call(&heart, commit, 8 * MS, 9 * MS);
        heart.park_at_ns(Park::Indefinite, 10 * MS);
        assert_eq!(
            heart.accounts.turns()[1].sum_ns,
            7 * MS,
            "6 ms once, then 1 ms"
        );
        assert_eq!(heart.accounts.door(commit).sum_ns, 4 * MS);
        assert_eq!(heart.accounts.door(present).sum_ns, 6 * MS);
    }

    /// One turn with one call past its bound: one line to queue. Turns that should each queue
    /// theirs are two seconds apart, past the coalescing window.
    fn one_line_turn(heart: &Heartbeat, at: u64) {
        heart.woke_at_ns(at);
        call(heart, key::<doors::PtyResize>(), at, at + 5 * MS);
        heart.park_at_ns(Park::Indefinite, at + 6 * MS);
    }

    /// RED (A3) — **a full ring or one the watchdog holds counts every line it refuses, and the
    /// next line printed says how many.**
    ///
    /// The window thread never waits for the ring: a push that finds it held is lost at once,
    /// like one that finds it full, and both are counted. The count reaches the log on the next
    /// line the watchdog writes, once.
    ///
    /// MUTATION: drop the `lost` count from `Accounts::offer`'s refusal arm and nothing is counted.
    #[test]
    fn a_full_ring_counts_losses_and_keeps_the_summary() {
        let heart = heart();
        for turn in 0..BUDGET_LINES_KEPT as u64 + 3 {
            one_line_turn(&heart, turn * 2_000 * MS);
        }
        {
            let _held = heart.accounts.ring().lock().expect("the ring");
            one_line_turn(&heart, 100_000 * MS);
        }
        let (taken, lost) = heart.accounts.take_lines();
        assert_eq!((taken.len(), lost), (BUDGET_LINES_KEPT, 4));
        let mut printed = 0;
        let written = said(taken, lost, &mut printed, 1);
        assert!(written[0].ends_with("refused=1 lost=4"), "{}", written[0]);
        assert!(written[1].ends_with("refused=1 lost=0"), "{}", written[1]);
        assert_eq!(printed, 4);
        // Emptied, it takes lines again, and nothing more was lost.
        one_line_turn(&heart, 200_000 * MS);
        let (taken, lost) = heart.accounts.take_lines();
        assert_eq!((taken.len(), lost), (1, 4));
        assert!(said(taken, lost, &mut printed, 1)[0].ends_with("lost=0"));
    }

    /// RED (A3) — **the exit summary is written from the atomics, and is whole after lines were
    /// lost.**
    ///
    /// Twenty turns, each with a call past its bound; four of their lines found no room. The
    /// summary still counts twenty turns and twenty calls and says four lines were lost — it does
    /// not depend on what the ring kept.
    ///
    /// MUTATION: print the lines the ring holds instead of the `lost` counter in
    /// `Accounts::summary` and the summary says sixteen.
    #[test]
    fn the_exit_summary_is_written_from_the_atomics_after_losses() {
        let heart = heart();
        for turn in 0..20 {
            one_line_turn(&heart, turn * 2_000 * MS);
        }
        let summary = heart.accounts.summary(3);
        assert_eq!(
            summary,
            [
                "Folio budget summary: wall count=20 max_us=6000 sum_us=120000 hist=4096us:20",
                "Folio budget summary: waits count=20 max_us=5000 sum_us=100000 hist=4096us:20",
                "Folio budget summary: unexplained count=20 max_us=1000 sum_us=20000 \
                 hist=512us:20",
                "Folio budget summary: delay count=0 max_us=0 sum_us=0 hist=-",
                "Folio budget summary: door=PtyResize row=12 count=20 max_us=5000 \
                 sum_us=100000 hist=4096us:20",
                "Folio budget summary: lost=4 refused=3",
            ]
        );
    }

    /// RED (A3) — **a call that never reaches `park` prints no budget line, and the watchdog's
    /// report is what it was.**
    ///
    /// A turn is accounted when it parks, so a call that never returns leaves nothing in the ring
    /// and nothing in the run's histograms; the watchdog owns it, and its verdict still names the
    /// door the thread is stuck in.
    ///
    /// MUTATION: account the turn in `admitted_enter_at` (call `accounts.close` there) and a line
    /// is queued for a turn that never ended.
    #[test]
    fn a_call_that_never_parks_prints_no_budget_line() {
        let heart = heart();
        heart.woke_at_ns(0);
        heart.beat_at(0);
        let _never_left = heart.admitted_enter_at(Station::PtyResize, 1);
        let (taken, lost) = heart.accounts.take_lines();
        assert_eq!((taken, lost), (vec![], 0));
        assert_eq!(heart.accounts.turns()[0].count, 0);
        assert_eq!(heart.accounts.door(key::<doors::PtyResize>()).count, 0);
        let mut watch = HangWatch::new(Duration::from_secs(5), Duration::from_secs(30));
        let mut ask = || bt_platform::hang::Answer::Silent;
        assert_eq!(watch.poll(0, heart.sample(), &mut ask), Verdict::Quiet);
        let Verdict::Hung { station, .. } = watch.poll(10_000, heart.sample(), &mut ask) else {
            panic!("the watchdog reports the stuck call");
        };
        assert_eq!(station, Station::PtyResize);
    }

    /// RED (A3) — **a turn's scheduling delay is reported apart from its wall time.**
    ///
    /// The loop parked until 10 ms and its next turn began at 13.5 ms: 3.5 ms late, which is the
    /// platform's (or the scheduler's), not the turn's. The turn's wall time is measured from its
    /// own start, so a descheduled thread is not blamed on anything the turn did. A turn woken
    /// early, or after a park with no deadline, has no delay at all.
    ///
    /// MUTATION: add the delay to the wall time in `Accounts::close` and the wall reads 23.5 ms.
    #[test]
    fn scheduling_delay_is_reported_apart_from_turn_time() {
        let heart = heart();
        heart.woke_at_ns(0);
        let parking = heart.until(heart.origin + Duration::from_millis(10));
        assert_eq!(parking, Park::Until(10));
        heart.park_at_ns(parking, 2 * MS);
        heart.woke_at_ns(13_500 * US);
        heart.park_at_ns(Park::Indefinite, 33_500 * US);
        let taken = lines(&heart);
        assert!(
            taken.contains(&Budget::Wall {
                wall_ns: 20 * MS,
                frame_ns: 16 * MS,
                waits_ns: 0,
                unexplained_ns: 20 * MS,
                delay_ns: Some(3_500 * US),
            }),
            "{taken:?}"
        );
        let [_, _, _, delay] = heart.accounts.turns();
        assert_eq!((delay.count, delay.sum_ns), (1, 3_500 * US));
        // An indefinite park owes nothing; a wake before the deadline is not late.
        heart.woke_at_ns(40 * MS);
        heart.park_at_ns(
            heart.until(heart.origin + Duration::from_millis(60)),
            41 * MS,
        );
        heart.woke_at_ns(50 * MS);
        heart.park_at_ns(Park::Indefinite, 51 * MS);
        assert_eq!(heart.accounts.turns()[3].count, 1);
        assert_eq!(
            BudgetLine {
                turn: 0,
                budget: Budget::Wall {
                    wall_ns: 20 * MS,
                    frame_ns: 16 * MS,
                    waits_ns: 0,
                    unexplained_ns: 20 * MS,
                    delay_ns: None,
                },
                suppressed: 3,
            }
            .line(0, 0),
            "Folio budget: turn=0 wall us=20000 frame_us=16000 waits_us=0 unexplained_us=20000 \
             delay_us=none suppressed=3 refused=0 lost=0"
        );
    }

    /// RED (A3, coordinator's ruling 2026-09-27) — **a busy second writes one line per trigger,
    /// and counts the turns it did not write.**
    ///
    /// Every turn below crosses all four triggers: 40 ms long against a 16 ms frame, two calls of
    /// 5 ms (10 ms of waits, each call past its bound), 30 ms unexplained. The first turn writes
    /// four lines. The turns that close inside the next second write none and are counted per
    /// trigger; the first turn that closes a second or more after those lines writes four again,
    /// each carrying the count. Every turn is still in the histograms, and nothing is lost.
    ///
    /// MUTATION: offer every line, ignoring `COALESCE_WINDOW` in `Accounts::offer`, and the ring
    /// takes sixteen lines and loses the rest.
    #[test]
    fn a_busy_second_writes_one_line_per_trigger_and_counts_the_rest() {
        let heart = heart();
        let resize = key::<doors::PtyResize>();
        let turns: u64 = 21;
        for turn in 0..turns {
            let at = turn * 50 * MS;
            heart.woke_at_ns(at);
            heart.beat_at(at / MS);
            call(&heart, resize, at + MS, at + 6 * MS);
            call(&heart, resize, at + 7 * MS, at + 12 * MS);
            heart.park_at_ns(Park::Indefinite, at + 40 * MS);
        }
        // The first lines closed at 40 ms; turn 20 closes at 1,040 ms, the first a second later.
        assert_eq!(20 * 50 * MS + 40 * MS - 40 * MS, nanos(COALESCE_WINDOW));
        let (taken, lost) = heart.accounts.take_lines();
        assert_eq!(lost, 0);
        let written: Vec<(u64, u64)> = taken
            .iter()
            .map(|line| (line.turn, line.suppressed))
            .collect();
        assert_eq!(
            written,
            [
                (1, 0),
                (1, 0),
                (1, 0),
                (1, 0),
                (21, 19),
                (21, 19),
                (21, 19),
                (21, 19)
            ]
        );
        assert!(
            matches!(taken[6].budget, Budget::Call { more: 1, .. }),
            "{:?}",
            taken[6]
        );
        assert!(
            taken[4]
                .line(0, 0)
                .contains(" suppressed=19 refused=0 lost=0"),
            "{}",
            taken[4].line(0, 0)
        );
        let [wall, waits, unexplained, _] = heart.accounts.turns();
        assert_eq!(
            (wall.count, waits.count, unexplained.count),
            (turns, turns, turns)
        );
        assert_eq!(heart.accounts.door(resize).count, 2 * turns);
    }

    /// RED (A3) — **a turn is measured against the shortest frame among the visible windows**
    /// (`T − t₀`), and against the default frame when none said.
    ///
    /// At 120 Hz a 10 ms turn has missed its frame; with no window's clock it has not.
    ///
    /// MUTATION: keep the longest frame (`fetch_max`) in `Accounts::frame` and the 120 Hz turn is
    /// measured against 60 Hz's.
    #[test]
    fn a_turn_is_measured_against_the_shortest_visible_frame() {
        let heart = heart();
        heart.woke_at_ns(0);
        heart.frame_interval(Duration::from_nanos(16_666_667));
        heart.frame_interval(Duration::from_nanos(8_333_333));
        heart.park_at_ns(Park::Indefinite, 10 * MS);
        assert!(
            matches!(
                lines(&heart).as_slice(),
                [
                    Budget::Wall {
                        frame_ns: 8_333_333,
                        ..
                    },
                    Budget::Unexplained { .. }
                ]
            ),
            "the 120 Hz window's frame"
        );
        heart.woke_at_ns(2_000 * MS);
        heart.park_at_ns(Park::Indefinite, 2_010 * MS);
        assert!(
            !lines(&heart)
                .iter()
                .any(|line| matches!(line, Budget::Wall { .. })),
            "no window said, so the default 16 ms frame"
        );
    }

    /// RED (A3) — **a call made before the loop's first turn is measured, and charges no turn.**
    ///
    /// The launch hand-over is admitted before the loop exists and the trace flush after it has
    /// returned (budget note §1.4, "stay"): no turn is running, so neither is a turn's wait, and
    /// each still has its own record.
    ///
    /// MUTATION: take a missing turn as one begun at zero in `Accounts::call` (drop its early
    /// return) and the next turn is charged the call.
    #[test]
    fn a_call_outside_every_turn_is_measured_and_charges_no_turn() {
        let heart = heart();
        let hand_over = key::<doors::LaunchHandOver>();
        call(&heart, hand_over, 0, 5 * MS);
        heart.woke_at_ns(10 * MS);
        heart.park_at_ns(Park::Indefinite, 11 * MS);
        assert_eq!(heart.accounts.door(hand_over).count, 1);
        assert_eq!(heart.accounts.turns()[1].sum_ns, 0);
        assert_eq!(lines(&heart), []);
    }

    /// RED (A3) — **an admitted call through the real `admitted` and the process's meter reaches
    /// its door's record and its turn's union**, both from the same two instants.
    ///
    /// The producer end of the seam: the instants are the ones `admitted` itself read, and the
    /// meter `start` installs turns them into the record.
    ///
    /// MUTATION: skip `accounts.call` in `admitted_leave_between` and the door's count stays zero.
    #[test]
    fn a_real_admission_is_one_call_on_its_doors_record_and_its_turns_union() {
        use bt_platform::admission::admitted;
        std::thread::spawn(|| {
            let heart: &'static Heartbeat = Box::leak(Box::new(heart()));
            let _ = bt_platform::admission::install_meter(super::super::ADMISSION_METER);
            super::super::TEST_HEART.with(|cell| cell.set(Some(heart)));
            assert!(bt_platform::admission::enter_window_thread());
            assert!(bt_platform::admission::loop_running());
            heart.woke();
            let spun = admitted::<doors::PtyResize, _>(|_token| {
                let began = Instant::now();
                while began.elapsed() < Duration::from_micros(50) {
                    std::hint::spin_loop();
                }
            });
            assert_eq!(spun, Ok(()));
            heart.park(Park::Indefinite);
            let tally = heart.accounts.door(key::<doors::PtyResize>());
            assert_eq!(tally.count, 1);
            assert!(tally.sum_ns >= 50 * US, "{tally:?}");
            assert_eq!(heart.accounts.turns()[1].sum_ns, tally.sum_ns);
            super::super::TEST_HEART.with(|cell| cell.set(None));
        })
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    }

    thread_local! {
        /// The counting clock's reads on this thread.
        static READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    fn counting_clock() -> Instant {
        READS.with(|reads| reads.set(reads.get() + 1));
        Instant::now()
    }

    fn reads() -> u64 {
        READS.with(std::cell::Cell::get)
    }

    /// RED (A3, §R-C 5) — **a parked loop reads no clock, and a turn's accounting adds none.**
    ///
    /// Counted on the heartbeat's injected clock. A turn is three reads — the wake, the pulse and
    /// the park — as before A3; its record is made from those. While the loop is parked nothing
    /// is read, the watchdog's takes and the exit summary included. An admitted call's `enter`
    /// reads once and its `leave` not at all: the station goes back on the call's own end instant,
    /// which `admitted` read — one read fewer than before A3.
    ///
    /// MUTATION: give the station back on `heart.now_ms()` in `admitted_leave` instead of the
    /// call's end and the call reads twice.
    #[test]
    fn a_parked_loop_reads_no_clock() {
        let mut heart = heart();
        heart.clock = counting_clock;
        let heart: &'static Heartbeat = Box::leak(Box::new(heart));
        super::super::TEST_HEART.with(|cell| cell.set(Some(heart)));
        let before = reads();
        heart.woke();
        heart.beat();
        let key = key::<doors::PtyResize>();
        let turn = reads();
        let cookie = (super::super::ADMISSION_METER.enter)(key);
        let start = Instant::now();
        let end = Instant::now();
        (super::super::ADMISSION_METER.leave)(key, cookie, start, end);
        assert_eq!(reads() - turn, 1, "an admitted call's meter reads once");
        heart.frame_interval(Duration::from_millis(16));
        heart.park(heart.until(Instant::now() + Duration::from_secs(1)));
        assert_eq!(reads() - before, 4, "wake, pulse, the call's enter, park");
        let parked = reads();
        let _ = heart.take_slow_holds();
        let _ = heart.accounts.take_lines();
        let _ = heart.accounts.summary(0);
        let _ = heart.sample();
        assert_eq!(reads(), parked, "a parked loop reads nothing");
        assert_eq!(heart.accounts.door(key).count, 1);
        assert_eq!(heart.accounts.turns()[0].count, 1);
        super::super::TEST_HEART.with(|cell| cell.set(None));
    }

    /// RED (A3) — **the slow-hold line keeps its meaning and its format.**
    ///
    /// A3 moved the heartbeat's clock to nanoseconds and put the turn's record ahead of the
    /// half-second threshold; the line its readers grep for is the same milliseconds, in the same
    /// words, in the same order.
    ///
    /// MUTATION: compute `held_ms` from nanoseconds rounded up (`div_ceil`) in `close_hold` and
    /// the line reads 1301 ms.
    #[test]
    fn the_slow_hold_line_is_unchanged() {
        let heart = heart();
        heart.woke_at_ns(0);
        heart.at_station(Station::Drain, 100);
        heart.park_at_ns(Park::Indefinite, 1_300 * MS + 400 * US);
        let (holds, dropped) = heart.take_slow_holds();
        assert_eq!(dropped, 0);
        let [hold] = holds.as_slice() else {
            panic!("one hold: {holds:?}")
        };
        assert_eq!(
            hold.line(),
            "Folio: the window thread held control for 1300 ms on turn 0 — drain_pty 1200 ms, \
             woken 100 ms · session age 1.300s, stall #1"
        );
    }
}
