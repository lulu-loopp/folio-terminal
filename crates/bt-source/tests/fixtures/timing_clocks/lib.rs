//! The handed-clock fixture (plan row 205): one product function per clock shape the handed-now
//! reading reads, each handed a clock and reading the real one again; what it admits; what is
//! not a clock; and what it refuses. Read by `timing::the_handed_clock_reading_reads_every_clock_shape`;
//! no target compiles it.

use std::time::{Duration, Instant};

/// A clock trait: a method answers an instant from its receiver alone.
pub trait Clock {
    fn now(&self) -> Instant;
}

/// A clock trait by its supertrait.
pub trait Paced: Clock {
    fn pause(&self, pause: Duration);
}

/// Not a clock: its instant is computed from an input.
pub trait Stamps {
    fn at(&self, turn: u32) -> Instant;
}

// ── handed, and reading again: each is found ──

pub fn by_impl_once(now: impl FnOnce() -> Instant) -> bool {
    now() > Instant::now()
}

pub fn by_dyn_fn(clock: &dyn Fn() -> Instant) -> Instant {
    clock().max(Instant::now())
}

pub fn by_dyn_fn_mut(tick: &mut dyn FnMut() -> Instant) -> Instant {
    tick().max(Instant::now())
}

pub fn by_boxed(clock: Box<dyn Fn() -> Instant + Send>) -> Instant {
    clock().max(Instant::now())
}

pub fn by_pointer(clock: fn() -> Instant) -> Instant {
    clock().max(Instant::now())
}

pub fn by_inline_generic<F: Fn() -> Instant>(clock: F) -> Instant {
    clock().max(Instant::now())
}

pub fn by_where_generic<F>(clock: &F) -> Instant
where
    F: Fn() -> Option<Instant>,
{
    clock().unwrap_or_else(Instant::now)
}

pub fn by_tuple((mut now, mut pause): (impl FnMut() -> Instant, impl FnMut(Duration))) -> bool {
    pause(Duration::ZERO);
    now() > Instant::now()
}

pub fn by_tuple_answer(clock: impl FnOnce() -> (Instant, u128)) -> bool {
    clock().0 > Instant::now()
}

pub fn by_trait_object(clock: &dyn Clock) -> bool {
    clock.now() > Instant::now()
}

pub fn by_supertrait<C: Paced + ?Sized>(clock: &C) -> bool {
    clock.now() > Instant::now()
}

/// Handing `Instant::now` on as a value is a reading deferred.
pub fn by_value_path(now: impl FnOnce() -> Instant) -> bool {
    forwards(Instant::now) || forwards(now)
}

pub fn by_inner_closure(now: impl Fn() -> Instant) -> Instant {
    let late = || Instant::now();
    now().max(late())
}

/// A struct parameter whose type argument is a clock-bounded parameter.
pub struct Ops<C> {
    clock: C,
}

pub fn by_carried<C: Clock>(ops: Ops<C>) -> bool {
    ops.clock.now() > Instant::now()
}

/// A field of a clock type: the owner's `self` methods are handed.
pub struct Beat {
    clock: fn() -> Instant,
}

impl Beat {
    /// No `self`: not handed, so the value `Instant::now` is no reading again.
    pub fn new() -> Self {
        Self {
            clock: Instant::now,
        }
    }

    pub fn late(&self) -> bool {
        (self.clock)() > Instant::now()
    }
}

/// An `impl` header bounding a parameter by a clock: its `self` methods are handed.
pub struct Pump<M> {
    media: M,
}

impl<M: Clock> Pump<M> {
    pub fn late(&self) -> bool {
        self.media.now() > Instant::now()
    }
}

// ── admitted, or not handed: nothing is found ──

/// Forwards its clock, calls it, and measures: a measurement's origin is admitted.
pub fn forwards(now: impl FnOnce() -> Instant) -> bool {
    let started = Instant::now();
    let due = now();
    started.elapsed() > Duration::ZERO || due.elapsed() > Duration::ZERO
}

/// A field `now` of a clock type, whose owner's method calls only it.
pub struct Bounded<'a> {
    now: &'a dyn Fn() -> Instant,
}

impl Bounded<'_> {
    pub fn over(&self, at: Instant) -> bool {
        (self.now)() >= at
    }
}

/// The real clock itself: its type carries no clock, so its `now` is not handed one.
pub struct Real;

impl Clock for Real {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

pub fn by_stamps(stamps: &dyn Stamps) -> bool {
    stamps.at(1) > Instant::now()
}

pub fn with_inputs(at: impl Fn(u32) -> Instant) -> bool {
    at(1) > Instant::now()
}

pub fn a_number_now(now: u64) -> u64 {
    now + Instant::now().elapsed().as_secs()
}

// ── refused ──

pub fn an_unread_type(clock: some_clock!()) -> bool {
    clock() > Instant::now()
}

pub fn a_now_in_a_pattern((now, _): Pair) -> Instant {
    now
}

pub fn an_unfollowed_clock(clock: &dyn elsewhere::Clock) -> bool {
    clock.now() > Instant::now()
}

mod away {
    impl super::Beat {
        pub fn away(&self) -> bool {
            true
        }
    }
}
