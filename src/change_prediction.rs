//! Analysis: predicting what a merge or a deletion would do, without changing
//! anything. Every prediction carries the exact frequencies and W afterwards.

use crate::huffman_tree::calculate_code_lengths;
use crate::predicted_changes::{DeletionPrediction, LeafChange, MergePrediction, OccurrenceRange, Respelling};
use crate::symbol_definition::SymbolDefinition;
use crate::symbol_stream::SymbolStream;

/// W (sum of frequency * code length) and every symbol's code length.
pub fn calculate_huffman_weight_and_code_lengths(symbol_frequencies: &[u64]) -> (u64, Vec<u32>) {
    let code_lengths = calculate_code_lengths(symbol_frequencies);
    let huffman_weight_in_bits = symbol_frequencies
        .iter()
        .zip(&code_lengths)
        .map(|(&frequency, &code_length)| frequency * code_length as u64)
        .sum();
    (huffman_weight_in_bits, code_lengths)
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

pub fn count_leaf_change(frequencies_before: &[u64], frequencies_after: &[u64]) -> LeafChange {
    let (removed_leaf_ids, added_leaf_ids) = list_leaves_removed_and_added(frequencies_before, frequencies_after);
    LeafChange { leaves_removed: removed_leaf_ids.len() as u64, leaves_added: added_leaf_ids.len() as u64 }
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
    let mut consumed_symbol_ids = Vec::new();
    let mut consumed_code_bits_before = 0u64;
    for &(start_position, end_position) in &merged_occurrence_ranges {
        for position in start_position..end_position {
            let consumed_symbol_id = stream.placed_symbol_ids[position as usize] as usize;
            symbol_frequencies_after[consumed_symbol_id] -= 1;
            consumed_code_bits_before += code_lengths_before[consumed_symbol_id] as u64;
            if !consumed_symbol_ids.contains(&consumed_symbol_id) {
                consumed_symbol_ids.push(consumed_symbol_id);
            }
        }
        symbol_frequencies_after[merged_symbol_id] += 1;
    }
    let (huffman_weight_after, code_lengths_after) =
        calculate_huffman_weight_and_code_lengths(&symbol_frequencies_after);
    let merged_code_bits_after = merged_occurrence_ranges.len() as u64 * code_lengths_after[merged_symbol_id] as u64;
    MergePrediction {
        candidate_index,
        merged_definition,
        merged_symbol_id,
        merged_occurrence_ranges,
        symbol_frequencies_after,
        huffman_weight_after,
        consumed_symbol_ids,
        consumed_code_bits_before,
        merged_code_bits_after,
    }
}

/// Spells the bits of `symbol_id` with known symbols (alive or dead) that are
/// not in `excluded_symbol_ids`: fewest pieces first, then fewest revived
/// (dead) symbols. Exact shortest path over the symbol's own bits.
/// `frequencies_for_revival_check` decides which symbols count as dead.
pub fn respell_with_fewest_pieces(
    stream: &SymbolStream,
    symbol_id: usize,
    excluded_symbol_ids: &[usize],
    frequencies_for_revival_check: &[u64],
) -> Option<Vec<u32>> {
    let definition = stream.symbol_definitions[symbol_id];
    let bit_length = definition.bit_length as usize;
    // best_from_bit[i] = ((pieces, revived), first symbol) to spell bits i..end.
    let mut best_from_bit: Vec<Option<((u32, u32), u32)>> = vec![None; bit_length + 1];
    best_from_bit[bit_length] = Some(((0, 0), 0));
    for start_bit in (0..bit_length).rev() {
        for piece_bit_length in 1..=bit_length - start_bit {
            let mask = if piece_bit_length == 64 { u64::MAX } else { (1u64 << piece_bit_length) - 1 };
            let piece_bits = (definition.bits >> (bit_length - start_bit - piece_bit_length)) & mask;
            let piece_definition = SymbolDefinition { bits: piece_bits, bit_length: piece_bit_length as u32 };
            let Some(piece_symbol_id) = stream.find_symbol_id(piece_definition) else { continue };
            if excluded_symbol_ids.contains(&piece_symbol_id) {
                continue;
            }
            if let Some(((pieces_after, revived_after), _)) = best_from_bit[start_bit + piece_bit_length] {
                let piece_is_revived = frequencies_for_revival_check[piece_symbol_id] == 0;
                let cost = (pieces_after + 1, revived_after + piece_is_revived as u32);
                if best_from_bit[start_bit].map_or(true, |(best_cost, _)| cost < best_cost) {
                    best_from_bit[start_bit] = Some((cost, piece_symbol_id as u32));
                }
            }
        }
    }
    best_from_bit[0]?;
    let mut replacement_symbol_ids = Vec::new();
    let mut start_bit = 0;
    while start_bit < bit_length {
        let piece_symbol_id = best_from_bit[start_bit].unwrap().1;
        replacement_symbol_ids.push(piece_symbol_id);
        start_bit += stream.symbol_definitions[piece_symbol_id as usize].bit_length as usize;
    }
    Some(replacement_symbol_ids)
}

/// Predicts deleting `deleted_symbol_ids` after `merge`: every remaining
/// placement of each is respelled without using any deleted symbol. None if
/// some deleted symbol cannot be spelled.
pub fn predict_deletion(
    stream: &SymbolStream,
    merge: &MergePrediction,
    deleted_symbol_ids: &[usize],
) -> Option<DeletionPrediction> {
    let mut symbol_frequencies_after = merge.symbol_frequencies_after.clone();
    let mut respellings = Vec::new();
    for &deleted_symbol_id in deleted_symbol_ids {
        let replacement_symbol_ids = respell_with_fewest_pieces(
            stream,
            deleted_symbol_id,
            deleted_symbol_ids,
            &merge.symbol_frequencies_after,
        )?;
        let deleted_frequency = symbol_frequencies_after[deleted_symbol_id];
        symbol_frequencies_after[deleted_symbol_id] = 0;
        for &replacement_symbol_id in &replacement_symbol_ids {
            symbol_frequencies_after[replacement_symbol_id as usize] += deleted_frequency;
        }
        respellings.push(Respelling { deleted_symbol_id, replacement_symbol_ids });
    }
    let (huffman_weight_after, _) = calculate_huffman_weight_and_code_lengths(&symbol_frequencies_after);
    Some(DeletionPrediction { respellings, symbol_frequencies_after, huffman_weight_after })
}
