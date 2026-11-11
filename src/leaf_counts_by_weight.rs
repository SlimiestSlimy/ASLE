//! How many leaves have each weight (frequency), kept sorted lightest first:
//! what the builder's hot path needs to know the exact total encoded symbol
//! bits after a candidate, without any tree (the user's design, 2026-11-11:
//! the tree is for encoding, not for hot lookups).
//!
//! The total comes from the two-queue method: the Huffman total depends only
//! on the multiset of weights, and with the leaf weights already sorted, the
//! merged nodes also come out lightest first, so two queues replace the heap.
//! Equal weights are kept as runs (weight, count) and paired up a whole run at
//! a time, so the work grows with the number of distinct weights, not leaves.
//!
//! The builder keeps the up-to-date counts in the stream and tries each
//! candidate on a working copy, refreshed from them before every candidate.

use crate::predicted_changes::FrequencyChange;

/// A run of equal weights: `count` nodes of weight `weight`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WeightRun {
    weight: u64,
    count: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeafCountsByWeight {
    runs_lightest_first: Vec<WeightRun>,
    leaf_count: u64,
}

impl LeafCountsByWeight {
    pub fn from_symbol_frequencies(symbol_frequencies: &[u64]) -> Self {
        let mut counts = Self::default();
        for &frequency in symbol_frequencies {
            counts.move_leaf(0, frequency);
        }
        counts
    }

    pub fn leaf_count(&self) -> usize {
        self.leaf_count as usize
    }

    /// One leaf's frequency goes from `frequency_before` to `frequency_after`
    /// (0 meaning: not a leaf).
    pub fn move_leaf(&mut self, frequency_before: u64, frequency_after: u64) {
        if frequency_before == frequency_after {
            return;
        }
        if frequency_before > 0 {
            let index = self.runs_lightest_first.partition_point(|run| run.weight < frequency_before);
            let run = &mut self.runs_lightest_first[index];
            debug_assert_eq!(run.weight, frequency_before, "the leaf's weight has a run");
            run.count -= 1;
            if run.count == 0 {
                self.runs_lightest_first.remove(index);
            }
            self.leaf_count -= 1;
        }
        if frequency_after > 0 {
            let index = self.runs_lightest_first.partition_point(|run| run.weight < frequency_after);
            match self.runs_lightest_first.get_mut(index) {
                Some(run) if run.weight == frequency_after => run.count += 1,
                _ => self.runs_lightest_first.insert(index, WeightRun { weight: frequency_after, count: 1 }),
            }
            self.leaf_count += 1;
        }
    }

    /// Applies a candidate's frequency changes; `frequencies_before` are the
    /// symbols' frequencies before it (a symbol past its end has frequency 0).
    pub fn apply_frequency_changes(&mut self, changes: &[FrequencyChange], frequencies_before: &[u64]) {
        for change in changes {
            let frequency_before = frequencies_before.get(change.symbol_id as usize).copied().unwrap_or(0);
            self.move_leaf(frequency_before, (frequency_before as i64 + change.delta) as u64);
        }
    }

    /// Exact sum over leaves of frequency * Huffman code length; a lone symbol
    /// costs 1 bit per occurrence.
    pub fn encoded_symbol_bits(&self) -> u64 {
        match self.leaf_count {
            0 => 0,
            1 => self.runs_lightest_first[0].weight,
            _ => two_queue_huffman_total(&self.runs_lightest_first, self.leaf_count),
        }
    }
}

/// The front of one queue: its runs from `next_run` on, the run at `next_run`
/// with `taken` of its nodes already used.
struct QueueFront {
    next_run: usize,
    taken: u64,
}

fn front_run(runs: &[WeightRun], front: &QueueFront) -> Option<WeightRun> {
    runs.get(front.next_run).map(|run| WeightRun { weight: run.weight, count: run.count - front.taken })
}

fn take_from(runs: &[WeightRun], front: &mut QueueFront, count: u64) {
    front.taken += count;
    if front.taken == runs[front.next_run].count {
        front.next_run += 1;
        front.taken = 0;
    }
}

/// Appends merged nodes; they arrive lightest first, so equal ones join the last run.
fn push_merged(merged: &mut Vec<WeightRun>, weight: u64, count: u64) {
    match merged.last_mut() {
        Some(last) if last.weight == weight => last.count += count,
        _ => merged.push(WeightRun { weight, count }),
    }
}

/// The Huffman total (sum of every merged node's weight) of `leaf_count >= 2`
/// leaves given as runs lightest first. Each step merges the two lightest
/// nodes; when the lightest run has two or more nodes, all its pairs merge at
/// once (the steps one by one would pair them the same way, as every merged
/// node is heavier than they are). The total does not depend on which of
/// several equal weights is taken.
fn two_queue_huffman_total(leaf_runs: &[WeightRun], leaf_count: u64) -> u64 {
    let mut merged_runs: Vec<WeightRun> = Vec::new();
    let mut leaf_front = QueueFront { next_run: 0, taken: 0 };
    let mut merged_front = QueueFront { next_run: 0, taken: 0 };
    let mut nodes_left = leaf_count;
    let mut total = 0u64;
    while nodes_left > 1 {
        let lightest_leaves = front_run(leaf_runs, &leaf_front);
        let lightest_merged = front_run(&merged_runs, &merged_front);
        let leaves_are_lightest = match (lightest_leaves, lightest_merged) {
            (Some(leaves), Some(merged)) => leaves.weight <= merged.weight,
            (Some(_), None) => true,
            _ => false,
        };
        let lightest = if leaves_are_lightest { lightest_leaves.unwrap() } else { lightest_merged.unwrap() };
        if lightest.count >= 2 {
            let pairs = lightest.count / 2;
            if leaves_are_lightest {
                take_from(leaf_runs, &mut leaf_front, 2 * pairs);
            } else {
                take_from(&merged_runs, &mut merged_front, 2 * pairs);
            }
            total += pairs * 2 * lightest.weight;
            nodes_left -= pairs;
            push_merged(&mut merged_runs, 2 * lightest.weight, pairs);
            continue;
        }
        // A single lightest node pairs with the next lightest, from either queue.
        if leaves_are_lightest {
            take_from(leaf_runs, &mut leaf_front, 1);
        } else {
            take_from(&merged_runs, &mut merged_front, 1);
        }
        let next_leaves = front_run(leaf_runs, &leaf_front);
        let next_merged = front_run(&merged_runs, &merged_front);
        let next_is_leaf = match (next_leaves, next_merged) {
            (Some(leaves), Some(merged)) => leaves.weight <= merged.weight,
            (Some(_), None) => true,
            _ => false,
        };
        let next_weight = if next_is_leaf {
            take_from(leaf_runs, &mut leaf_front, 1);
            next_leaves.unwrap().weight
        } else {
            let weight = next_merged.unwrap().weight;
            take_from(&merged_runs, &mut merged_front, 1);
            weight
        };
        let merged_weight = lightest.weight + next_weight;
        total += merged_weight;
        nodes_left -= 1;
        push_merged(&mut merged_runs, merged_weight, 1);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::huffman_tree::calculate_encoded_symbol_bits;

    /// A small deterministic generator for the tests (splitmix64).
    struct TestRandom(u64);
    impl TestRandom {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
        fn below(&mut self, bound: u64) -> u64 {
            self.next() % bound
        }
    }

    /// Random frequency tables of every shape (few or many distinct weights,
    /// long runs of equal weights): the total equals a heap build's.
    #[test]
    fn total_matches_a_heap_build() {
        let mut random = TestRandom(3);
        for _ in 0..20000 {
            let symbol_count = 1 + random.below(300) as usize;
            let largest = [1, 2, 3, 8, 100, 100000][random.below(6) as usize];
            let frequencies: Vec<u64> = (0..symbol_count).map(|_| random.below(largest + 1)).collect();
            let counts = LeafCountsByWeight::from_symbol_frequencies(&frequencies);
            assert_eq!(counts.encoded_symbol_bits(), calculate_encoded_symbol_bits(&frequencies), "{frequencies:?}");
            assert_eq!(counts.leaf_count(), frequencies.iter().filter(|&&f| f > 0).count());
        }
    }

    /// Frequency changes applied to the counts give the same counts as
    /// counting the changed table from scratch.
    #[test]
    fn changes_match_a_recount() {
        let mut random = TestRandom(4);
        let mut frequencies: Vec<u64> = (0..100).map(|_| random.below(20)).collect();
        let mut counts = LeafCountsByWeight::from_symbol_frequencies(&frequencies);
        for _ in 0..20000 {
            let symbol_id = random.below(frequencies.len() as u64 + 1) as usize;
            if symbol_id == frequencies.len() {
                frequencies.push(0);
            }
            let delta = if frequencies[symbol_id] > 0 && random.below(2) == 0 {
                -((1 + random.below(frequencies[symbol_id])) as i64)
            } else {
                (1 + random.below(30)) as i64
            };
            counts.apply_frequency_changes(&[FrequencyChange { symbol_id: symbol_id as u32, delta }], &frequencies);
            frequencies[symbol_id] = (frequencies[symbol_id] as i64 + delta) as u64;
            assert_eq!(counts, LeafCountsByWeight::from_symbol_frequencies(&frequencies));
        }
    }
}
