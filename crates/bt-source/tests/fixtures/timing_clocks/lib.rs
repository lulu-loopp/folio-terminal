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

/// Forwards its clock, calls it, and measures: the measurement's origin is admitted, and
/// `due.elapsed()` reads the real clock again (row 205 review F2), so it is found, once.
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

pub fn an_unread_type(clock: clock_of!(Instant)) -> bool {
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

// ── the review fold (row 205): carriers, `.elapsed()`, trait defaults, instants anywhere, the
// wall clock, and the branches each reading takes ──

use std::time::SystemTime;

/// A carrier handed as a parameter (F1).
pub fn by_carrier(beat: &Beat) -> Instant {
    let _ = beat;
    Instant::now()
}

pub fn by_optional_carrier(beat: Option<Box<Beat>>) -> Instant {
    let _ = beat;
    Instant::now()
}

/// A carrier one level down: `Outer` carries a clock through its `Beat`.
pub struct Outer {
    beat: Beat,
}

impl Outer {
    pub fn late(&self) -> Instant {
        Instant::now()
    }
}

/// Two levels down: the fixed point.
pub struct Outermost {
    outers: Vec<Outer>,
}

impl Outermost {
    pub fn late(&self) -> Instant {
        Instant::now()
    }
}

impl Beat {
    /// `Self` in a block whose `self` is handed.
    pub fn same(other: &Self) -> Instant {
        let _ = other;
        Instant::now()
    }

    /// `.elapsed()` on an instant that is not a fresh origin (F2).
    pub fn by_stored_elapsed(&self, at: Instant) -> bool {
        at.elapsed() > Duration::ZERO
    }
}

mod deep {
    pub struct Held {
        pub clock: fn() -> std::time::Instant,
    }
}

/// A carrier named by a module path.
pub fn by_pathed_carrier(held: &deep::Held) -> Instant {
    let _ = held;
    Instant::now()
}

/// A carrier through a `use`, renamed.
use self::deep::Held as Kept;

pub fn by_used_carrier(held: &Kept) -> Instant {
    let _ = held;
    Instant::now()
}

/// A clock trait two supertraits down, with a default method (F3).
pub trait Timed: Paced {
    fn due_in(&self, wait: Duration) -> Instant {
        Instant::now() + wait
    }
}

pub trait Deep: Timed {}

pub fn by_deep_supertrait(clock: &dyn Deep) -> bool {
    clock.now() > Instant::now()
}

/// A `now` holding an instant in a type the old reading did not read (F4).
pub fn by_boxed_now(now: Box<Instant>) -> bool {
    *now > Instant::now()
}

pub fn by_borrowed_option_now(now: &Option<Instant>) -> bool {
    now.is_some_and(|at| at > Instant::now())
}

pub fn by_now_slice(now: &[Instant]) -> bool {
    now[0] > Instant::now()
}

/// The wall clock (Q1).
pub fn by_wall_now(now: SystemTime) -> bool {
    now > SystemTime::now()
}

pub fn by_wall_clock(clock: impl Fn() -> SystemTime) -> bool {
    clock() > SystemTime::now()
}

/// Pointers, arrays, slices.
pub fn by_pointer_to_clock(clock: *const dyn Fn() -> Instant) -> Instant {
    let _ = clock;
    Instant::now()
}

pub fn by_array(clocks: [fn() -> Instant; 2]) -> Instant {
    clocks[0]().max(Instant::now())
}

pub fn by_slice(clocks: &[&dyn Clock]) -> bool {
    clocks[0].now() > Instant::now()
}

/// An `impl` whose clock bound is in its `where` clause.
pub struct Pumped<M> {
    media: M,
}

impl<M> Pumped<M>
where
    M: Clock,
{
    pub fn late(&self) -> bool {
        self.media.now() > Instant::now()
    }
}

/// A struct's own clock-bounded parameter makes it a carrier.
pub struct Bounded2<C: Clock> {
    clock: C,
}

pub fn by_bounded_carrier(held: &Bounded2<Real>) -> Instant {
    let _ = held;
    Instant::now()
}

/// An enum and a union that carry a clock.
pub enum Either {
    Ticking(fn() -> Instant),
    Still,
}

pub fn by_enum_carrier(either: &Either) -> Instant {
    let _ = either;
    Instant::now()
}

pub union Raw {
    clock: fn() -> Instant,
    bits: usize,
}

pub fn by_union_carrier(raw: &Raw) -> Instant {
    let _ = raw;
    Instant::now()
}

/// A closure handed a clock, in a function handed none: its reading is found.
pub fn by_closure_parameter() -> Instant {
    let read = |clock: &dyn Fn() -> Instant| clock().max(Instant::now());
    read(&Instant::now)
}

/// A nested `fn` handed a clock, in a function handed none.
pub fn by_nested_fn() -> Instant {
    fn inner(clock: fn() -> Instant) -> Instant {
        clock().max(Instant::now())
    }
    inner(Instant::now)
}

// ── the review fold: admitted, or not handed ──

/// A closure whose `now` is a number, in a handed body: it clears nothing, so its reading is
/// found (round 2, RR-F1).
pub fn a_number_now_in_a_handed_body(clock: impl Fn() -> Instant) -> Instant {
    let shift = |now: u64| Instant::now() + Duration::from_secs(now);
    clock().max(shift(1))
}

/// A macro type naming no clock (`Token![,]`-like).
pub fn with_a_token_type(comma: Token![,]) -> Instant {
    let _ = comma;
    Instant::now()
}

// ── the review fold: refused ──

pub fn a_fn_macro_type(clock: boxed!(dyn Fn() -> Instant)) -> bool {
    let _ = clock;
    true
}

pub fn a_clock_trait_macro_type(clock: boxed!(dyn Clock)) -> bool {
    let _ = clock;
    true
}

pub fn a_where_on_a_type<T>(value: T) -> bool
where
    Vec<T>: Clock,
{
    let _ = value;
    true
}

pub fn an_unfollowed_carrier(held: &crate::gone::Beat) -> bool {
    let _ = held;
    true
}

/// Across packages: the `other` fixture's clock trait and carrier.
pub fn by_other_trait(clock: &dyn other::Ticker) -> bool {
    clock.tick() > Instant::now()
}

pub fn by_other_carrier(held: &other::Carried) -> Instant {
    let _ = held;
    Instant::now()
}

// ── the review fold: a path that leads outside, or an alias, carries no clock ──

/// An outside crate's type named like a carrier: by the language's rules, `elsewhere` here is an
/// outside crate, whose types carry no first-party clock.
pub fn with_an_outside_type(held: &elsewhere::Beat) -> Instant {
    let _ = held;
    Instant::now()
}

/// An associated type named like a carrier: not seen (the guard's "does not see" list).
pub fn with_an_associated_type<C: Stamps>(held: &C::Beat) -> Instant {
    let _ = held;
    Instant::now()
}

/// A type alias of a carrier is a type, resolved, but its target is not followed.
pub type Ticks = Beat;

pub fn with_an_alias(held: &Ticks) -> Instant {
    let _ = held;
    Instant::now()
}

// ── R8: a reading counts by kind ──

/// Handed the wall clock, reads the monotonic one: a first reading, not found.
pub fn wall_handed_reads_instant(now: SystemTime) -> Instant {
    let _ = now;
    Instant::now()
}

/// Handed an instant, reads the wall clock: a first reading, not found.
pub fn instant_handed_reads_wall(now: Instant) -> SystemTime {
    let _ = now;
    SystemTime::now()
}

/// Handed a wall clock, measures an instant with `.elapsed()`: the monotonic clock, not found.
pub fn wall_clock_handed_measures(clock: impl Fn() -> SystemTime, at: Instant) -> bool {
    clock() > SystemTime::UNIX_EPOCH && at.elapsed() > Duration::ZERO
}

/// Handed both kinds, reads both again: found twice.
pub fn handed_both(now: (Instant, SystemTime)) -> bool {
    now.0 > Instant::now() || now.1 > SystemTime::now()
}

/// A carrier of the wall clock: its wall reading is found, its monotonic one is not.
pub struct WallBeat {
    clock: fn() -> SystemTime,
}

pub fn by_wall_carrier(held: &WallBeat) -> bool {
    let _ = held;
    Instant::now().elapsed() > Duration::ZERO || SystemTime::now() > SystemTime::UNIX_EPOCH
}

/// A wall clock trait: its wall reading is found, its monotonic ones are not.
pub trait Wall {
    fn wall(&self) -> SystemTime;
}

pub fn by_wall_trait(clock: &dyn Wall) -> bool {
    clock.wall() <= SystemTime::now() || Instant::now().elapsed() > Duration::ZERO
}

// ── round 2: a closure keeps its body's clock, `.elapsed()` reads its receiver's kind, an
// origin is matched by scope ──

/// A wall clock closure in a body handed an instant: the instant reading in it is found.
pub fn instant_body_wall_closure(now: Instant) -> bool {
    let read =
        |wall: &dyn Fn() -> SystemTime| wall() > SystemTime::UNIX_EPOCH && Instant::now() > now;
    read(&SystemTime::now)
}

/// A closure handed a wall `now` in a body handed an instant: both its readings are found.
pub fn both_inside_a_closure(now: Instant) -> bool {
    let earlier = now;
    let judge = |now: SystemTime| now <= SystemTime::now() || Instant::now() > earlier;
    judge(SystemTime::UNIX_EPOCH)
}

/// Handed a wall time, measures a wall time with `.elapsed()`: a wall reading, found.
pub fn wall_elapsed(now: SystemTime, at: SystemTime) -> bool {
    let _ = now;
    at.elapsed().is_ok()
}

/// A stored wall time, of no written type at the `.elapsed()`.
pub struct Stamped {
    at: SystemTime,
}

/// Handed a wall time, measures a receiver whose type is not written: both kinds, found.
pub fn untyped_elapsed(now: SystemTime, held: &Stamped) -> bool {
    let _ = now;
    held.at.elapsed().is_ok()
}

/// An origin in an inner block admits its own `.elapsed()`, not a same-named binding outside it.
pub fn origin_in_an_inner_block(now: Instant, started: Instant) -> bool {
    let _ = now;
    {
        let started = Instant::now();
        let _ = started.elapsed();
    }
    started.elapsed() > Duration::ZERO
}
