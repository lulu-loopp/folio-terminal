#![expect(
    clippy::indexing_slicing,
    reason = "Fenwick indices are bounded by construction; checked access would obscure the algorithm"
)]

#[derive(Clone, Debug, Default)]
pub struct HeightTree {
    heights: Vec<i64>,
    fenwick: Vec<i64>,
}

impl HeightTree {
    pub fn rebuild(&mut self, heights: impl IntoIterator<Item = i64>) {
        self.heights = heights.into_iter().collect();
        self.fenwick = vec![0; self.heights.len() + 1];
        for index in 0..self.heights.len() {
            let value = self.heights[index];
            self.add(index, value);
        }
    }

    fn add(&mut self, index: usize, delta: i64) {
        let mut i = index + 1;
        while i < self.fenwick.len() {
            self.fenwick[i] += delta;
            i += i & i.wrapping_neg();
        }
    }

    pub fn set(&mut self, index: usize, value: i64) {
        let delta = value - self.heights[index];
        self.heights[index] = value;
        self.add(index, delta);
    }

    pub fn push(&mut self, value: i64) {
        if self.fenwick.is_empty() {
            self.fenwick.push(0);
        }
        self.heights.push(value);
        let one_based = self.heights.len();
        let low_bit = one_based & one_based.wrapping_neg();
        let range_start = one_based - low_bit;
        let covered_before = self.prefix_sum(one_based - 1) - self.prefix_sum(range_start);
        self.fenwick.push(covered_before + value);
    }

    pub fn prefix_sum(&self, count: usize) -> i64 {
        let mut i = count.min(self.heights.len());
        let mut sum = 0;
        while i > 0 {
            sum += self.fenwick[i];
            i &= i - 1;
        }
        sum
    }

    pub fn total(&self) -> i64 {
        self.prefix_sum(self.heights.len())
    }

    /// Locate the item containing a zero-based offset in O(log n). Values must be non-negative.
    pub fn index_at_offset(&self, offset: i64) -> Option<usize> {
        if self.heights.is_empty() || offset < 0 || offset >= self.total() {
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
        Some(index.min(self.heights.len() - 1))
    }

    pub fn get(&self, index: usize) -> Option<i64> {
        self.heights.get(index).copied()
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
}
