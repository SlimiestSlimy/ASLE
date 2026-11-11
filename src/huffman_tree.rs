//! Huffman math used by the builder's decisions: exact encoded symbol bits and
//! code lengths. (The container uses an order-preserving code tree instead;
//! see order_preserving_code_tree.)
//!
//! Encoded symbol bits = sum over all leaves of symbol frequency * leaf depth
//! = the exact number of bits the Huffman-encoded symbols take up.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Encoded symbol bits: sum over leaves of frequency * depth, computed as the
/// sum of all merged node weights while building the tree. A lone symbol
/// still costs 1 bit per occurrence.
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
/// 1 for a lone symbol. Ties are broken by node id, so this is deterministic.
pub fn calculate_code_lengths(symbol_frequencies: &[u64]) -> Vec<u32> {
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
