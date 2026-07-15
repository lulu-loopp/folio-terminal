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
}
