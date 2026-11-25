#![expect(
    clippy::indexing_slicing,
    reason = "Fenwick indices are bounded by construction; checked access would obscure the algorithm"
)]

/// Heights in order, with prefix sums and offset lookup in O(log n).
///
/// **Its front can be dropped without a rebuild** (0.4.10 plan row 105). A full pane loses its
/// oldest line every time it gains one, and a Fenwick tree has no cheap way to remove its first
/// item: every node's range is counted from the start. So a dropped item stays stored, at height
/// zero, behind a count of such items (`dead`), and every index this type takes or answers is
/// counted from the first item still standing. Zero heights add nothing to any prefix, so the
/// sums need no correction; once the dead part is more than half of what is stored, the tree is
/// rebuilt from the living part, which keeps the storage under twice the items. A rebuild adds
/// every living item again, O(n log n), spread over the n drops since the last one; with the
/// drop's own O(log n) zeroing, a dropped item costs O(log n) amortized.
#[derive(Clone, Debug, Default)]
pub struct HeightTree {
    heights: Vec<i64>,
    fenwick: Vec<i64>,
    /// How many stored items at the front have been dropped and stand at height zero.
    dead: usize,
}

impl HeightTree {
    pub fn rebuild(&mut self, heights: impl IntoIterator<Item = i64>) {
        self.heights = heights.into_iter().collect();
        self.dead = 0;
        self.fenwick = vec![0; self.heights.len() + 1];
        for index in 0..self.heights.len() {
            let value = self.heights[index];
            self.add(index, value);
        }
    }

    /// `delta` at a stored position.
    fn add(&mut self, stored: usize, delta: i64) {
        let mut i = stored + 1;
        while i < self.fenwick.len() {
            self.fenwick[i] += delta;
            i += i & i.wrapping_neg();
        }
    }

    pub fn set(&mut self, index: usize, value: i64) {
        let stored = index + self.dead;
        let delta = value - self.heights[stored];
        self.heights[stored] = value;
        self.add(stored, delta);
    }

    pub fn push(&mut self, value: i64) {
        if self.fenwick.is_empty() {
            self.fenwick.push(0);
        }
        self.heights.push(value);
        let one_based = self.heights.len();
        let low_bit = one_based & one_based.wrapping_neg();
        let range_start = one_based - low_bit;
        let covered_before = self.stored_prefix(one_based - 1) - self.stored_prefix(range_start);
        self.fenwick.push(covered_before + value);
    }

    /// Drop the first `count` items. Everything after them keeps its height and moves `count`
    /// places towards the front.
    pub fn drop_front(&mut self, count: usize) {
        debug_assert!(
            count <= self.len(),
            "dropping {count} of {} items",
            self.len()
        );
        for stored in self.dead..self.dead + count {
            let height = self.heights[stored];
            self.heights[stored] = 0;
            self.add(stored, -height);
        }
        self.dead += count;
        if self.dead * 2 > self.heights.len() {
            let living = self.heights.split_off(self.dead);
            self.rebuild(living);
        }
    }

    /// How many items there are, the dropped ones not counted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.heights.len() - self.dead
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many items are stored, the dropped ones that wait for the next compaction included.
    /// A measurement for the tests that hold the storage under twice the items.
    #[doc(hidden)]
    #[must_use]
    pub fn stored_len(&self) -> usize {
        self.heights.len()
    }

    /// The sum of the first `count` stored items, dropped ones included (they are zero).
    fn stored_prefix(&self, count: usize) -> i64 {
        let mut i = count.min(self.heights.len());
        let mut sum = 0;
        while i > 0 {
            sum += self.fenwick[i];
            i &= i - 1;
        }
        sum
    }

    pub fn prefix_sum(&self, count: usize) -> i64 {
        self.stored_prefix(count.min(self.len()) + self.dead)
    }

    pub fn total(&self) -> i64 {
        self.stored_prefix(self.heights.len())
    }

    /// Locate the item containing a zero-based offset in O(log n). Values must be non-negative.
    pub fn index_at_offset(&self, offset: i64) -> Option<usize> {
        if self.is_empty() || offset < 0 || offset >= self.total() {
            return None;
        }
        let mut index = 0usize;
        let mut sum = 0i64;
        let mut bit = 1usize << (usize::BITS - self.heights.len().leading_zeros() - 1);
        while bit != 0 {
            let next = index + bit;
            if next <= self.heights.len() && sum + self.fenwick[next] <= offset {
                index = next;
                sum += self.fenwick[next];
            }
            bit >>= 1;
        }
        // Every dropped item is zero and stands before the offset's item, so the search has
        // passed all of them: `index` is at least `dead`.
        Some(index.min(self.heights.len() - 1) - self.dead)
    }

    pub fn get(&self, index: usize) -> Option<i64> {
        if index >= self.len() {
            return None;
        }
        self.heights.get(index + self.dead).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_tree_updates_without_float_drift() {
        let mut tree = HeightTree::default();
        tree.rebuild([i64::MAX / 8, i64::MAX / 8, 42]);
        let before = tree.total();
        tree.set(2, 84);
        assert_eq!(tree.total(), before + 42);
    }

    #[test]
    fn incremental_push_matches_rebuild_prefixes() {
        let values = [7, 3, 11, 5, 2, 13, 17];
        let mut incremental = HeightTree::default();
        for value in values {
            incremental.push(value);
        }
        let mut rebuilt = HeightTree::default();
        rebuilt.rebuild(values);
        for count in 0..=values.len() {
            assert_eq!(incremental.prefix_sum(count), rebuilt.prefix_sum(count));
        }
        assert_eq!(incremental.index_at_offset(0), Some(0));
        assert_eq!(incremental.index_at_offset(6), Some(0));
        assert_eq!(incremental.index_at_offset(7), Some(1));
        assert_eq!(
            incremental.index_at_offset(incremental.total() - 1),
            Some(6)
        );
        assert_eq!(incremental.index_at_offset(incremental.total()), None);
    }

    /// **A tree whose front was dropped answers as a tree built from what is left**, at every
    /// index and every offset, through pushes and compactions alike (0.4.10 plan row 105).
    #[test]
    fn dropping_the_front_answers_as_a_tree_built_from_the_rest() {
        let mut tree = HeightTree::default();
        let mut living = Vec::new();
        let mut next = 1_i64;
        for round in 0..40 {
            for _ in 0..7 {
                tree.push(next % 5 + 1);
                living.push(next % 5 + 1);
                next += 1;
            }
            let dropped = (round % 4) + 3;
            tree.drop_front(dropped);
            living.drain(..dropped);

            let mut fresh = HeightTree::default();
            fresh.rebuild(living.iter().copied());
            assert_eq!(tree.len(), living.len());
            assert_eq!(tree.total(), fresh.total());
            for count in 0..=living.len() {
                assert_eq!(tree.prefix_sum(count), fresh.prefix_sum(count));
            }
            for (index, height) in living.iter().enumerate() {
                assert_eq!(tree.get(index), Some(*height));
            }
            assert_eq!(tree.get(living.len()), None);
            for offset in 0..=fresh.total() {
                assert_eq!(tree.index_at_offset(offset), fresh.index_at_offset(offset));
            }
            assert!(
                tree.stored_len() <= 2 * tree.len() + 1,
                "{} stored for {} items",
                tree.stored_len(),
                tree.len()
            );
        }
    }
}
