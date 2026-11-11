//! Analysis: predicting what a merge or a leaf removal would do, without
//! changing anything.
//! Every prediction carries the exact frequencies and encoded symbol bits
//! afterwards.

use crate::huffman_tree::calculate_code_lengths;
use crate::predicted_changes::{LeafRemovalPrediction, MergePrediction, OccurrenceRange};
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

/// A leaf keeps its place while its code is not longer than its own bits
/// (break-even leaves stay: they are building blocks for longer symbols).
pub fn leaf_code_is_not_longer_than_its_bits(bit_length: u32, code_length: u32) -> bool {
    code_length <= bit_length
}

/// Leaves whose code is longer than their own bits, biggest loss first:
/// loss = frequency * (code length - bit length); ties: higher frequency,
/// then depth-first order of the bits.
pub fn find_non_saving_leaves(stream: &SymbolStream, code_lengths: &[u32]) -> Vec<usize> {
    let mut non_saving_leaf_ids: Vec<usize> = (0..stream.symbol_definitions.len())
        .filter(|&symbol_id| stream.symbol_frequencies[symbol_id] > 0)
        .filter(|&symbol_id| {
            !leaf_code_is_not_longer_than_its_bits(stream.symbol_definitions[symbol_id].bit_length, code_lengths[symbol_id])
        })
        .collect();
    let loss = |symbol_id: usize| {
        stream.symbol_frequencies[symbol_id]
            * (code_lengths[symbol_id] - stream.symbol_definitions[symbol_id].bit_length) as u64
    };
    non_saving_leaf_ids.sort_by(|&first, &second| {
        loss(second)
            .cmp(&loss(first))
            .then(stream.symbol_frequencies[second].cmp(&stream.symbol_frequencies[first]))
            .then_with(|| {
                let (a, b) = (stream.symbol_definitions[first], stream.symbol_definitions[second]);
                stream.shared_bit_sequence.compare_slices_depth_first(
                    a.first_bit_offset,
                    a.bit_length as u64,
                    b.first_bit_offset,
                    b.bit_length as u64,
                )
            })
    });
    non_saving_leaf_ids
}

/// Spells the bits of `symbol_id` with other live leaves, using the fewest
/// code bits under the current code lengths (exact shortest path over the
/// symbol's own bits); ties: fewer pieces. None if no spelling exists.
pub fn respell_with_fewest_code_bits(stream: &SymbolStream, symbol_id: usize, code_lengths: &[u32]) -> Option<Vec<u32>> {
    let definition = stream.symbol_definitions[symbol_id];
    let bit_length = definition.bit_length as usize;
    // best_from_bit[i] = ((code bits, pieces), first piece) to spell bits i..end.
    let mut best_from_bit: Vec<Option<((u64, u32), u32)>> = vec![None; bit_length + 1];
    best_from_bit[bit_length] = Some(((0, 0), 0));
    for start_bit in (0..bit_length).rev() {
        for piece_bit_length in 1..=bit_length - start_bit {
            let piece = SymbolDefinition {
                first_bit_offset: definition.first_bit_offset + start_bit as u64,
                bit_length: piece_bit_length as u32,
            };
            let Some(piece_symbol_id) = stream.find_symbol_id(piece) else { continue };
            if piece_symbol_id == symbol_id || stream.symbol_frequencies[piece_symbol_id] == 0 {
                continue;
            }
            if let Some(((code_bits_after, pieces_after), _)) = best_from_bit[start_bit + piece_bit_length] {
                let cost = (code_bits_after + code_lengths[piece_symbol_id] as u64, pieces_after + 1);
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

/// Predicts removing `symbol_id`; None if it cannot be respelled.
pub fn predict_leaf_removal(
    stream: &SymbolStream,
    symbol_id: usize,
    code_lengths_before: &[u32],
) -> Option<LeafRemovalPrediction> {
    let replacement_symbol_ids = respell_with_fewest_code_bits(stream, symbol_id, code_lengths_before)?;
    let removed_frequency = stream.symbol_frequencies[symbol_id];
    let mut symbol_frequencies_after = stream.symbol_frequencies.clone();
    symbol_frequencies_after[symbol_id] = 0;
    for &replacement_symbol_id in &replacement_symbol_ids {
        symbol_frequencies_after[replacement_symbol_id as usize] += removed_frequency;
    }
    let (encoded_symbol_bits_after, code_lengths_after) =
        calculate_encoded_symbol_bits_and_code_lengths(&symbol_frequencies_after);
    let replacement_code_bits_after = removed_frequency
        * replacement_symbol_ids.iter().map(|&id| code_lengths_after[id as usize] as u64).sum::<u64>();
    Some(LeafRemovalPrediction {
        removed_symbol_id: symbol_id,
        removed_frequency,
        replacement_symbol_ids,
        symbol_frequencies_after,
        encoded_symbol_bits_after,
        removed_code_bits_before: removed_frequency * code_lengths_before[symbol_id] as u64,
        replacement_code_bits_after,
    })
}
