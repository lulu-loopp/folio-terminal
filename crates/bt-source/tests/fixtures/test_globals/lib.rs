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
}
