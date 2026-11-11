//! Huffman trees: every symbol's code length, once per builder round and once
//! for the container, built with the two-queue method from the leaves sorted
//! by frequency (no heap). The builder's hot path never builds a tree; it reads
//! totals from leaf_counts_by_weight. The heap versions stay as the test
//! reference.
//!
//! Encoded symbol bits = sum over all leaves of symbol frequency * leaf depth
//! = the exact number of bits the Huffman-encoded symbols take up.

#[cfg(test)]
use std::cmp::Reverse;
#[cfg(test)]
use std::collections::BinaryHeap;

/// Encoded symbol bits: sum over leaves of frequency * depth, computed as the
/// sum of all merged node weights while building the tree. A lone symbol
/// still costs 1 bit per occurrence.
#[cfg(test)] // the reference leaf_counts_by_weight is tested against
pub fn calculate_encoded_symbol_bits(symbol_frequencies: &[u64]) -> u64 {
    let mut lightest_first: BinaryHeap<Reverse<u64>> =
        symbol_frequencies.iter().copied().filter(|&frequency| frequency > 0).map(Reverse).collect();
    if lightest_first.len() == 1 {
        return lightest_first.pop().unwrap().0;
    }
    let mut encoded_symbol_bits = 0;
    while lightest_first.len() > 1 {
        let Reverse(lightest_weight) = lightest_first.pop().unwrap();
        let Reverse(second_lightest_weight) = lightest_first.pop().unwrap();
        encoded_symbol_bits += lightest_weight + second_lightest_weight;
        lightest_first.push(Reverse(lightest_weight + second_lightest_weight));
    }
    encoded_symbol_bits
}

/// Huffman code length (leaf depth) of every symbol; 0 for frequency 0, and
/// 1 for a lone symbol. Two queues: the leaves sorted by (frequency, symbol
/// id), and the merged nodes, which come out lightest first in creation order.
/// Each step takes the lighter front, the leaf on equal weights: exactly the
/// order of a heap keyed by (weight, node id) with leaves numbered first, so
/// the code lengths equal the heap build's (calculate_code_lengths_with_heap).
pub fn calculate_code_lengths(symbol_frequencies: &[u64]) -> Vec<u32> {
    let mut code_lengths = vec![0u32; symbol_frequencies.len()];
    let mut leaf_symbol_ids: Vec<u32> =
        (0..symbol_frequencies.len() as u32).filter(|&symbol_id| symbol_frequencies[symbol_id as usize] > 0).collect();
    if leaf_symbol_ids.len() == 1 {
        code_lengths[leaf_symbol_ids[0] as usize] = 1;
    }
    if leaf_symbol_ids.len() <= 1 {
        return code_lengths;
    }
    leaf_symbol_ids.sort_unstable_by_key(|&symbol_id| (symbol_frequencies[symbol_id as usize], symbol_id));
    let leaf_count = leaf_symbol_ids.len();
    // Node ids: leaves 0..leaf_count in sorted order, then merged nodes as created.
    let mut parent_node_id = vec![u32::MAX; 2 * leaf_count - 1];
    let mut merged_weights: Vec<u64> = Vec::with_capacity(leaf_count - 1);
    let mut next_leaf = 0usize;
    let mut next_merged = 0usize;
    let mut take_lightest = |merged_weights: &Vec<u64>| -> (u64, usize) {
        let leaf_weight = leaf_symbol_ids.get(next_leaf).map(|&symbol_id| symbol_frequencies[symbol_id as usize]);
        let merged_weight = merged_weights.get(next_merged).copied();
        match (leaf_weight, merged_weight) {
            (Some(leaf), Some(merged)) if leaf <= merged => {
                next_leaf += 1;
                (leaf, next_leaf - 1)
            }
            (Some(leaf), None) => {
                next_leaf += 1;
                (leaf, next_leaf - 1)
            }
            (_, Some(merged)) => {
                next_merged += 1;
                (merged, leaf_count + next_merged - 1)
            }
            (None, None) => unreachable!("two nodes are left while merging"),
        }
    };
    for merged_index in 0..leaf_count - 1 {
        let (lightest_weight, lightest_node_id) = take_lightest(&merged_weights);
        let (second_weight, second_node_id) = take_lightest(&merged_weights);
        let merged_node_id = (leaf_count + merged_index) as u32;
        parent_node_id[lightest_node_id] = merged_node_id;
        parent_node_id[second_node_id] = merged_node_id;
        merged_weights.push(lightest_weight + second_weight);
    }
    // Parents always have higher ids than children; the root is the last node.
    let node_count = 2 * leaf_count - 1;
    let mut node_depth = vec![0u32; node_count];
    for node_id in (0..node_count - 1).rev() {
        node_depth[node_id] = node_depth[parent_node_id[node_id] as usize] + 1;
    }
    for (node_id, &symbol_id) in leaf_symbol_ids.iter().enumerate() {
        code_lengths[symbol_id as usize] = node_depth[node_id];
    }
    code_lengths
}

#[cfg(test)] // the reference calculate_code_lengths is tested against
/// Huffman code length (leaf depth) of every symbol; 0 for frequency 0, and
/// 1 for a lone symbol. Ties are broken by node id, so this is deterministic.
pub fn calculate_code_lengths_with_heap(symbol_frequencies: &[u64]) -> Vec<u32> {
    let mut code_lengths = vec![0u32; symbol_frequencies.len()];
    let leaf_symbol_ids: Vec<usize> =
        (0..symbol_frequencies.len()).filter(|&symbol_id| symbol_frequencies[symbol_id] > 0).collect();
    if leaf_symbol_ids.len() == 1 {
        code_lengths[leaf_symbol_ids[0]] = 1;
    }
    if leaf_symbol_ids.len() <= 1 {
        return code_lengths;
    }
    // Node ids: leaves first (in symbol id order), then internal nodes as created.
    let mut parent_node_id = vec![usize::MAX; 2 * leaf_symbol_ids.len() - 1];
    let mut lightest_first: BinaryHeap<Reverse<(u64, usize)>> = leaf_symbol_ids
        .iter()
        .enumerate()
        .map(|(node_id, &symbol_id)| Reverse((symbol_frequencies[symbol_id], node_id)))
        .collect();
    let mut next_node_id = leaf_symbol_ids.len();
    while lightest_first.len() > 1 {
        let Reverse((lightest_weight, lightest_node_id)) = lightest_first.pop().unwrap();
        let Reverse((second_lightest_weight, second_lightest_node_id)) = lightest_first.pop().unwrap();
        parent_node_id[lightest_node_id] = next_node_id;
        parent_node_id[second_lightest_node_id] = next_node_id;
        lightest_first.push(Reverse((lightest_weight + second_lightest_weight, next_node_id)));
        next_node_id += 1;
    }
    // Parents always have higher ids than children; the root is the last node.
    let mut node_depth = vec![0u32; next_node_id];
    for node_id in (0..next_node_id - 1).rev() {
        node_depth[node_id] = node_depth[parent_node_id[node_id]] + 1;
    }
    for (node_id, &symbol_id) in leaf_symbol_ids.iter().enumerate() {
        code_lengths[symbol_id] = node_depth[node_id];
    }
    code_lengths
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Random frequency tables of every shape (many ties, few, long runs):
    /// the two-queue code lengths equal the heap build's, symbol by symbol.
    #[test]
    fn two_queue_code_lengths_equal_the_heap_build() {
        let mut state = 5u64;
        let mut below = |bound: u64| {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            (z ^ (z >> 31)) % bound
        };
        for _ in 0..20000 {
            let symbol_count = 1 + below(300) as usize;
            let largest = [1, 2, 3, 8, 100, 100000][below(6) as usize];
            let frequencies: Vec<u64> = (0..symbol_count).map(|_| below(largest + 1)).collect();
            assert_eq!(calculate_code_lengths(&frequencies), calculate_code_lengths_with_heap(&frequencies), "{frequencies:?}");
        }
    }
}
