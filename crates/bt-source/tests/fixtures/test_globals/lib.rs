// The process-globals guard's fixture (T-TEST-PROCESS-GLOBALS): one case per shape of the
// grammar, each beside the shape that is not one, the classes its register checks, held and
// broken, and a global reached only through a product function. Read by bt-source's
// `test_globals` test; never compiled.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Product code's own counter: a test reaches it only through `product_bump`, a product
/// function, so the walk does not see it (the stated blind spot).
static PRODUCT_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn product_bump() {
    PRODUCT_COUNT.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{LazyLock, Mutex, OnceLock};

    /// A unit lock: the remedy, never a global.
    static TURN: Mutex<()> = Mutex::new(());

    /// Locked in every test that reaches it: a `lock:TURN` row holds.
    static LOCKED: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn locked_writes() {
        let _turn = TURN.lock();
        LOCKED.store(1, Ordering::Relaxed);
    }

    #[test]
    fn locked_reads() {
        let _turn = TURN.lock();
        assert_eq!(LOCKED.load(Ordering::Relaxed), 1);
    }

    /// Locked in one test and not in the other: its `lock:TURN` row is broken.
    static HALF_LOCKED: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn half_locked_writes() {
        let _turn = TURN.lock();
        HALF_LOCKED.store(1, Ordering::Relaxed);
    }

    #[test]
    fn half_locked_reads_without_the_turn() {
        let _ = HALF_LOCKED.load(Ordering::Relaxed);
    }

    /// G1: read by two tests, on no row.
    static SHARED: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn shared_once() {
        assert_eq!(SHARED.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn shared_twice() {
        let _ = SHARED.load(Ordering::Relaxed);
    }

    /// G2: written by one test, on no row.
    static WRITTEN: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn written() {
        WRITTEN.fetch_add(1, Ordering::Relaxed);
    }

    /// G2: a lock taken to change what it holds is a write.
    static WRITTEN_BY_LOCK: Mutex<Vec<u8>> = Mutex::new(Vec::new());

    #[test]
    fn written_by_lock() {
        WRITTEN_BY_LOCK.lock().unwrap().push(1);
    }

    /// Not a hit: one test, and it only reads. Its row is stale.
    static READ_ONCE: AtomicBool = AtomicBool::new(false);

    #[test]
    fn read_once() {
        let _ = READ_ONCE.load(Ordering::Relaxed);
    }

    /// G1 through the call graph: named only in a helper two tests call.
    static HELPED: AtomicUsize = AtomicUsize::new(0);

    fn bump_helped() {
        HELPED.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn helped_once() {
        bump_helped();
    }

    #[test]
    fn helped_twice() {
        bump_helped();
    }

    /// G1 through the call graph: a static declared in a helper's own body.
    fn cached() -> &'static str {
        static CACHE: OnceLock<String> = OnceLock::new();
        CACHE.get_or_init(|| "fixed".to_owned())
    }

    #[test]
    fn cached_once() {
        let _ = cached();
    }

    #[test]
    fn cached_twice() {
        let _ = cached();
    }

    /// G2: an undecided use (handed by reference) is read as a write.
    static UNDECIDED: AtomicUsize = AtomicUsize::new(0);

    fn count(cell: &AtomicUsize) -> usize {
        cell.load(Ordering::Relaxed)
    }

    #[test]
    fn undecided() {
        let _ = count(&UNDECIDED);
    }

    /// One test writes, another reads: a `single writer` row holds.
    static ONE_WRITER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn one_writer_writes() {
        ONE_WRITER.store(2, Ordering::Relaxed);
    }

    #[test]
    fn one_writer_is_read() {
        let _ = ONE_WRITER.load(Ordering::Relaxed);
    }

    /// Two tests write: its `single writer` row is broken.
    static TWO_WRITERS: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn two_writers_first() {
        TWO_WRITERS.store(1, Ordering::Relaxed);
    }

    #[test]
    fn two_writers_second() {
        TWO_WRITERS.store(2, Ordering::Relaxed);
    }

    /// A `LazyLock` of a fixed list, read by two tests: a `write-once` row holds.
    static FAMILIES: LazyLock<Vec<&'static str>> = LazyLock::new(|| vec!["one", "two"]);

    #[test]
    fn families_once() {
        assert!(FAMILIES.contains(&"one"));
    }

    #[test]
    fn families_twice() {
        let _ = FAMILIES.len();
    }

    /// Stored by one test: its `write-once` row is broken.
    static RESET: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn reset() {
        RESET.store(0, Ordering::Relaxed);
    }

    /// Read by two tests that are not on the ignored list: its `run=alone` row is broken.
    static ALONE: AtomicBool = AtomicBool::new(false);

    #[test]
    fn alone_once() {
        let _ = ALONE.load(Ordering::Relaxed);
    }

    #[test]
    fn alone_twice() {
        let _ = ALONE.load(Ordering::Relaxed);
    }

    /// Written by one test: a by-hand `monotone` row is taken as written.
    static GROWS: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn grows() {
        GROWS.fetch_add(1, Ordering::Relaxed);
    }

    thread_local! {
        /// Not a global: each test's thread has its own.
        static PER_THREAD: Cell<usize> = const { Cell::new(0) };
    }

    #[test]
    fn per_thread_once() {
        PER_THREAD.with(|cell| cell.set(1));
    }

    #[test]
    fn per_thread_twice() {
        PER_THREAD.with(|cell| cell.set(2));
    }

    /// Not a global: a static declared inside a test body is that test's.
    #[test]
    fn its_own_static() {
        static MINE: AtomicUsize = AtomicUsize::new(0);
        MINE.fetch_add(1, Ordering::Relaxed);
    }

    /// The blind spot: two tests reach `PRODUCT_COUNT` only through a product function.
    #[test]
    fn through_product_once() {
        super::product_bump();
    }

    #[test]
    fn through_product_twice() {
        super::product_bump();
    }

    /// A helper the call graph cannot reach by type, whose method writes a global.
    struct Poker;

    static POKED: AtomicUsize = AtomicUsize::new(0);

    impl Poker {
        fn poke(&self) {
            POKED.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Refused by file and line: a call on a receiver the call graph cannot type, whose
    /// candidate reaches a global.
    #[test]
    fn a_global_reached_through_an_untyped_receiver() {
        let ask = |poker| poker.poke();
        let _ = ask;
    }

    // ── review fold (2026-10-24) ──────────────────────────────────────────────

    /// A `single writer` row broken by an implicit call: one test stores it, and another
    /// test's guard stores it again when it drops at the end of the test (H1).
    static GUARDED: AtomicUsize = AtomicUsize::new(0);

    struct Reset;

    impl Drop for Reset {
        fn drop(&mut self) {
            GUARDED.store(0, Ordering::Relaxed);
        }
    }

    #[test]
    fn guarded_writes() {
        GUARDED.store(1, Ordering::Relaxed);
    }

    #[test]
    fn guarded_is_reset_by_a_drop() {
        let _reset = Reset;
    }

    /// Refused: an implicit impl that writes a global, whose type no test's reach names.
    static ORPHANED: AtomicUsize = AtomicUsize::new(0);

    struct Orphan;

    impl Drop for Orphan {
        fn drop(&mut self) {
            ORPHANED.store(1, Ordering::Relaxed);
        }
    }

    /// G2 through a foreign callback: a function written in an item-position macro's tokens,
    /// run for a type a test names (`objc2::define_class!`'s shape).
    static PRESSED: AtomicUsize = AtomicUsize::new(0);

    callback_class!(
        #[unsafe(super(NSView))]
        struct Pressable;

        impl Pressable {
            #[unsafe(method(mouseDown:))]
            fn mouse_down(&self) {
                PRESSED.fetch_add(1, Ordering::SeqCst);
            }
        }
    );

    #[test]
    fn pressing() {
        let _ = Pressable::alloc();
    }

    /// `static mut` written through an index and through a field (G2 each), and read through
    /// a field by one test (no hit) (H2).
    struct Point {
        x: u32,
    }

    static mut TALLIES: [u32; 2] = [0, 0];
    static mut SPOT: Point = Point { x: 0 };
    static mut LOOKED_AT: Point = Point { x: 0 };

    #[test]
    fn tallies_by_index() {
        unsafe { TALLIES[0] += 1 }
    }

    #[test]
    fn spot_by_field() {
        unsafe { SPOT.x = 1 }
    }

    #[test]
    fn looked_at_by_field() {
        let _ = unsafe { LOOKED_AT.x };
    }

    /// Refused: a path the call graph places nowhere whose last segment is a global's name
    /// (H3).
    static LOST: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn a_global_through_a_path_nothing_places() {
        self::missing::LOST.fetch_add(1, Ordering::Relaxed);
    }

    /// One test on two `cfg` arms: the arm that forgets `TURN` breaks the `lock:TURN` row,
    /// whichever arm is read last.
    static ARMED_TWICE: AtomicUsize = AtomicUsize::new(0);

    #[cfg(not(windows))]
    #[test]
    fn on_each_arm() {
        ARMED_TWICE.store(2, Ordering::Relaxed);
    }

    #[cfg(windows)]
    #[test]
    fn on_each_arm() {
        let _turn = TURN.lock();
        ARMED_TWICE.store(1, Ordering::Relaxed);
    }

    #[test]
    fn on_each_arm_is_read() {
        let _turn = TURN.lock();
        let _ = ARMED_TWICE.load(Ordering::Relaxed);
    }

    /// Two unit locks of one name: a row naming it bare is broken, and a row spelling it by its
    /// module path holds.
    mod gated {
        pub static GATE_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    }

    mod elsewhere {
        pub static GATE_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    }

    static GATED: AtomicUsize = AtomicUsize::new(0);
    static GATED_TOO: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn gated_writes() {
        let _turn = gated::GATE_TURN.lock();
        GATED.store(1, Ordering::Relaxed);
        GATED_TOO.store(1, Ordering::Relaxed);
    }

    #[test]
    fn gated_reads() {
        let _turn = gated::GATE_TURN.lock();
        let _ = GATED.load(Ordering::Relaxed) + GATED_TOO.load(Ordering::Relaxed);
    }

    /// Refused: a global a glob `use` makes nameable, named in macro arguments that do not
    /// parse.
    static GLOBBED: AtomicUsize = AtomicUsize::new(0);

    mod globbed {
        use super::*;

        #[test]
        fn a_global_named_through_a_glob_in_an_unparsed_macro() {
            odd_macro!(=> GLOBBED <=);
        }
    }

    /// An array of cells is the cell: written by index (G2); its twin read by index by one test
    /// (no hit).
    static SLOTS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
    static SLOTS_READ: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

    #[test]
    fn slots_by_index() {
        SLOTS[1].fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn slots_read_by_index() {
        let _ = SLOTS_READ[0].load(Ordering::Relaxed);
    }

    /// Read by two ignored tests listed `run=alone`: its `run=alone` row holds.
    static ALONE_HELD: AtomicBool = AtomicBool::new(false);

    #[test]
    #[ignore = "run=alone"]
    fn alone_held_once() {
        let _ = ALONE_HELD.load(Ordering::Relaxed);
    }

    #[test]
    #[ignore = "run=alone"]
    fn alone_held_twice() {
        let _ = ALONE_HELD.load(Ordering::Relaxed);
    }
}
