//! Analysis: predicting what a merge would do, without changing anything.
//! Every prediction carries the exact frequencies and encoded symbol bits
//! afterwards.

use crate::huffman_tree::calculate_code_lengths;
use crate::predicted_changes::{MergePrediction, OccurrenceRange};
use crate::symbol_definition::SymbolDefinition;
use crate::symbol_stream::SymbolStream;

/// Encoded symbol bits (sum of frequency * code length) and every symbol's
/// code length.
pub fn calculate_encoded_symbol_bits_and_code_lengths(symbol_frequencies: &[u64]) -> (u64, Vec<u32>) {
    let code_lengths = calculate_code_lengths(symbol_frequencies);
    let encoded_symbol_bits = symbol_frequencies
        .iter()
        .zip(&code_lengths)
        .map(|(&frequency, &code_length)| frequency * code_length as u64)
        .sum();
    (encoded_symbol_bits, code_lengths)
}

/// Ids of the leaves that disappear and the leaves that appear going from
/// `frequencies_before` to `frequencies_after` (which may be one longer).
pub fn list_leaves_removed_and_added(frequencies_before: &[u64], frequencies_after: &[u64]) -> (Vec<usize>, Vec<usize>) {
    let mut removed_leaf_ids = Vec::new();
    let mut added_leaf_ids = Vec::new();
    for symbol_id in 0..frequencies_after.len() {
        let was_leaf = symbol_id < frequencies_before.len() && frequencies_before[symbol_id] > 0;
        let is_leaf = frequencies_after[symbol_id] > 0;
        if was_leaf && !is_leaf {
            removed_leaf_ids.push(symbol_id);
        }
        if !was_leaf && is_leaf {
            added_leaf_ids.push(symbol_id);
        }
    }
    (removed_leaf_ids, added_leaf_ids)
}

/// Predicts replacing each of `merged_occurrence_ranges` with one symbol.
pub fn predict_merge(
    stream: &SymbolStream,
    candidate_index: usize,
    merged_definition: SymbolDefinition,
    merged_occurrence_ranges: Vec<OccurrenceRange>,
    code_lengths_before: &[u32],
) -> MergePrediction {
    let mut symbol_frequencies_after = stream.symbol_frequencies.clone();
    let merged_symbol_id = match stream.find_symbol_id(merged_definition) {
        Some(existing_symbol_id) => existing_symbol_id,
        None => {
            symbol_frequencies_after.push(0);
            symbol_frequencies_after.len() - 1
        }
    };
    let mut consumed_code_bits_before = 0u64;
    for &(start_position, end_position) in &merged_occurrence_ranges {
        for position in start_position..end_position {
            let consumed_symbol_id = stream.placed_symbol_ids[position as usize] as usize;
            symbol_frequencies_after[consumed_symbol_id] -= 1;
            consumed_code_bits_before += code_lengths_before[consumed_symbol_id] as u64;
        }
        symbol_frequencies_after[merged_symbol_id] += 1;
    }
    let (encoded_symbol_bits_after, code_lengths_after) =
        calculate_encoded_symbol_bits_and_code_lengths(&symbol_frequencies_after);
    let merged_code_bits_after = merged_occurrence_ranges.len() as u64 * code_lengths_after[merged_symbol_id] as u64;
    MergePrediction {
        candidate_index,
        merged_definition,
        merged_symbol_id,
        merged_occurrence_ranges,
        symbol_frequencies_after,
        encoded_symbol_bits_after,
        consumed_code_bits_before,
        merged_code_bits_after,
    }
}
