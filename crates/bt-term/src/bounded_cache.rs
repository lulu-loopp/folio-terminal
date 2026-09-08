//! **A map that knows what it costs, and lets go of the oldest thing when it
//! costs too much** (review row R1-8, adversarial review 2026-09-08).
//!
//! # What was wrong, in one sentence
//!
//! Three decoded-pixel stores were `HashMap`s that were inserted into and never
//! emptied: the window's peek cache, the decoration worker's local-path memo and
//! the window's animations. Every distinct file a pointer rested on left its
//! pixels in one of them for as long as the process lived, and a documentation
//! page full of screenshots put every screenshot in it at once.
//!
//! # Why one type for all three
//!
//! They are three different maps holding three different values, and the rule
//! they were all missing is the same rule: a store whose keys come from outside
//! this program needs a ceiling, and a ceiling needs somebody to let go when it
//! is reached. Written three times it would be three chances to count the bytes
//! differently; written once it is one counter, maintained on insert and on
//! evict, and one test that the counter tells the truth.
//!
//! # Recency is readable through a shared reference, and that is deliberate
//!
//! The frame path asks these caches through `&self` — a picture is looked up
//! while the window is drawing, and a lookup that needed `&mut` would either
//! push the borrow up through every drawing method or would silently not count
//! as a use. Neither is acceptable: a picture nobody has drawn since the session
//! began and one being drawn thirty times a second must not look the same to an
//! eviction. So the clock and each entry's last reading are [`Cell`]s, which is
//! the honest shape of "reading this map changes what it will evict next" —
//! there is no interior state a caller can observe except through the eviction
//! order, and there is no thread sharing: every one of these caches lives on one
//! thread.
//!
//! # The cost of an eviction
//!
//! Choosing a victim is a walk of the map. It happens only on an insert that
//! goes over budget, and the number of entries is itself bounded by the budget,
//! so the walk is bounded — but it is a walk, and a cache tuned to hold hundreds
//! of thousands of tiny entries would want a linked order instead of this. None
//! of the three is: they hold pictures.

use std::borrow::Borrow;
use std::cell::Cell;
use std::collections::HashMap;
use std::hash::Hash;

/// **What one entry costs**, in bytes, not counting the map's own bookkeeping.
///
/// Answered by the value rather than measured, because the thing worth counting
/// is the pixels — an `Arc<[u8]>` of eight megabytes is eight megabytes however
/// small the struct pointing at it is.
pub trait Weighed {
    /// The bytes this value keeps alive.
    fn bytes_held(&self) -> u64;
}

/// **What an entry costs before its value is counted at all.**
///
/// A refusal and a request in flight hold no pixels, and a store that counted
/// only pixels would let a million of them accumulate under a budget it never
/// reached — which is the same defect one size down. This is the map's own share
/// per entry: the hash slot, the boxed key and the enum around the value, rounded
/// to something that is honest at this scale rather than exact.
const ENTRY_OVERHEAD_BYTES: u64 = 128;

/// One value, and when it was last read.
#[derive(Debug)]
struct Slot<V> {
    value: V,
    /// The clock reading at the last [`BoundedCache::get`] or insert. `Cell`
    /// because a read through `&self` is a use — see the module note.
    used: Cell<u64>,
    /// What this entry was counted as when it went in. Kept rather than
    /// recomputed so that a value whose weight changed under the map cannot make
    /// the running total drift: what is added on insert is exactly what is taken
    /// away on eviction.
    weight: u64,
}

/// A map with a byte ceiling and least-recently-used eviction. See the module
/// note.
#[derive(Debug)]
pub struct BoundedCache<K, V> {
    entries: HashMap<K, Slot<V>>,
    /// Ticks once per read and once per insert; the entry holding the smallest
    /// reading is the least recently used one.
    clock: Cell<u64>,
    held: u64,
    budget: u64,
}

impl<K, V> BoundedCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Weighed,
{
    /// A cache that will hold at most `budget` bytes.
    #[must_use]
    pub fn with_budget(budget: u64) -> Self {
        Self {
            entries: HashMap::new(),
            clock: Cell::new(0),
            held: 0,
            budget,
        }
    }

    /// **What this cache is holding**, counted as it was inserted.
    #[must_use]
    pub fn bytes_held(&self) -> u64 {
        self.held
    }

    /// The ceiling this cache was built with.
    #[must_use]
    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// How many entries are in it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it is holding nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// **Read one entry, and count the reading.** See the module note on why
    /// this takes `&self`.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let slot = self.entries.get(key)?;
        slot.used.set(self.tick());
        Some(&slot.value)
    }

    /// Whether a key is present, **without** counting a reading.
    ///
    /// The one caller is "have I already asked for this", which is a question
    /// about the ledger and not a use of the pixels.
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.contains_key(key)
    }

    /// Every value, mutably, in no particular order. Reading them this way is
    /// not a use: a sweep over the whole map says nothing about which entry
    /// anybody wanted.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.values_mut().map(|slot| &mut slot.value)
    }

    /// Every key and value, in no particular order, and likewise not a use.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(key, slot)| (key, &slot.value))
    }

    /// **Put one entry in, and let go of the oldest until the budget is met.**
    ///
    /// Replacing a key is not an eviction of it: the old weight comes off the
    /// total and the new one goes on, and the entry keeps its place at the front
    /// of the recency order because an insert is the most recent thing that has
    /// happened to it.
    pub fn insert(&mut self, key: K, value: V) {
        let weight = ENTRY_OVERHEAD_BYTES.saturating_add(value.bytes_held());
        let used = self.tick();
        if let Some(previous) = self.entries.insert(
            key.clone(),
            Slot {
                value,
                used: Cell::new(used),
                weight,
            },
        ) {
            self.held = self.held.saturating_sub(previous.weight);
        }
        self.held = self.held.saturating_add(weight);
        self.evict_until_within_budget(&key);
    }

    /// Take one entry out, whatever it weighs.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let slot = self.entries.remove(key)?;
        self.held = self.held.saturating_sub(slot.weight);
        Some(slot.value)
    }

    /// Empty it.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.held = 0;
    }

    fn tick(&self) -> u64 {
        let now = self.clock.get().saturating_add(1);
        self.clock.set(now);
        now
    }

    /// Drop least-recently-used entries until the total is within budget.
    ///
    /// `keep` is the key that was just inserted, and it is spared: a value
    /// larger than the whole budget would otherwise be evicted by its own
    /// arrival, which is a cache that answers `None` to everything it was just
    /// given. What happens instead is that it stands alone, and the next insert
    /// takes it.
    fn evict_until_within_budget(&mut self, keep: &K) {
        while self.held > self.budget {
            let Some(oldest) = self
                .entries
                .iter()
                .filter(|(key, _)| *key != keep)
                .min_by_key(|(_, slot)| slot.used.get())
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            self.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value that weighs whatever it says it weighs.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Pixels(u64);

    impl Weighed for Pixels {
        fn bytes_held(&self) -> u64 {
            self.0
        }
    }

    /// RED — **a cache with a budget holds at most the budget, and what it lets
    /// go of is the oldest thing in it** (review row R1-8).
    ///
    /// RED EVIDENCE (2026-09-08), against the three `HashMap`s this type
    /// replaces, which had no counter and no eviction at all:
    ///
    /// ```text
    /// ten megabytes of pictures into a four megabyte cache: it is holding 10487040
    /// ```
    ///
    /// MUTATION: return early from `evict_until_within_budget` and the first
    /// assertion goes red with everything that was ever inserted still in it.
    #[test]
    fn a_full_cache_lets_go_of_the_oldest_and_never_of_the_budget() {
        const MEGABYTE: u64 = 1024 * 1024;
        let mut cache: BoundedCache<String, Pixels> = BoundedCache::with_budget(4 * MEGABYTE);
        for index in 0..10 {
            cache.insert(format!("picture {index}"), Pixels(MEGABYTE));
        }
        assert!(
            cache.bytes_held() <= cache.budget(),
            "ten megabytes of pictures into a four megabyte cache: it is holding {}",
            cache.bytes_held(),
        );
        // Three fit under the budget once each entry's own overhead is counted,
        // and they are the last three: the oldest went first.
        assert_eq!(cache.len(), 3);
        for index in 0..7 {
            assert!(
                !cache.contains_key(&format!("picture {index}")),
                "picture {index} was the oldest at some point and is gone",
            );
        }
        for index in 7..10 {
            assert!(cache.contains_key(&format!("picture {index}")));
        }
    }

    /// PIN — **a reading is a use**: the entry a caller keeps asking for is the
    /// one that survives, whatever order the entries went in.
    #[test]
    fn the_entry_that_is_being_read_is_not_the_one_evicted() {
        const MEGABYTE: u64 = 1024 * 1024;
        let mut cache: BoundedCache<String, Pixels> = BoundedCache::with_budget(3 * MEGABYTE);
        cache.insert("the one on screen".to_owned(), Pixels(MEGABYTE));
        for index in 0..6 {
            // Read before every insert, exactly as a frame does.
            assert_eq!(cache.get("the one on screen"), Some(&Pixels(MEGABYTE)));
            cache.insert(format!("passer by {index}"), Pixels(MEGABYTE));
        }
        assert!(
            cache.contains_key("the one on screen"),
            "the picture the window kept drawing is the picture it kept",
        );
    }

    /// PIN — **the counter tells the truth**: a replaced key is counted once,
    /// and a removed one is counted not at all.
    #[test]
    fn the_running_total_follows_every_door() {
        let mut cache: BoundedCache<u32, Pixels> = BoundedCache::with_budget(1024 * 1024);
        cache.insert(1, Pixels(1000));
        assert_eq!(cache.bytes_held(), ENTRY_OVERHEAD_BYTES + 1000);
        cache.insert(1, Pixels(2000));
        assert_eq!(
            cache.bytes_held(),
            ENTRY_OVERHEAD_BYTES + 2000,
            "a replacement is not a second entry",
        );
        assert_eq!(cache.remove(&1), Some(Pixels(2000)));
        assert_eq!(cache.bytes_held(), 0);
        assert!(cache.is_empty());
    }

    /// PIN — **a value larger than the whole budget stands alone rather than
    /// evicting itself.**
    #[test]
    fn one_picture_larger_than_the_budget_is_still_answered() {
        let mut cache: BoundedCache<u32, Pixels> = BoundedCache::with_budget(1024);
        cache.insert(1, Pixels(512));
        cache.insert(2, Pixels(4096));
        assert_eq!(cache.get(&2), Some(&Pixels(4096)));
        assert_eq!(cache.len(), 1, "and everything else made room for it");
    }
}
