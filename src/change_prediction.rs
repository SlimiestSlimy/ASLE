//! Analysis: predicting what a merge or a leaf removal would do, without
//! changing anything.
//! Every prediction carries its frequency changes (worked out from the
//! candidate's own data) and the exact encoded symbol bits afterwards, read
//! from a working copy of the up-to-date leaf counts by weight, refreshed
//! before each candidate (no trees: see leaf_counts_by_weight).

use crate::huffman_tree::calculate_code_lengths;
use crate::leaf_counts_by_weight::{LeafCountsByWeight, MergedRunsBuffer};
use crate::predicted_changes::{
    count_into_frequency_changes, FrequencyChange, LeafRemovalPrediction, MergePrediction, OccurrenceRange,
};
use crate::symbol_definition::SymbolDefinition;
use crate::symbol_stream::SymbolStream;

/// Ids of the leaves that disappear and the leaves that appear with `changes`,
/// in symbol id order.
pub fn list_leaves_removed_and_added(frequencies_before: &[u64], changes: &[FrequencyChange]) -> (Vec<usize>, Vec<usize>) {
    let mut removed_leaf_ids = Vec::new();
    let mut added_leaf_ids = Vec::new();
    for change in changes {
        let symbol_id = change.symbol_id as usize;
        let frequency_before = frequencies_before.get(symbol_id).copied().unwrap_or(0);
        let frequency_after = (frequency_before as i64 + change.delta) as u64;
        if frequency_before > 0 && frequency_after == 0 {
            removed_leaf_ids.push(symbol_id);
        }
        if frequency_before == 0 && frequency_after > 0 {
            added_leaf_ids.push(symbol_id);
        }
    }
    (removed_leaf_ids, added_leaf_ids)
}

/// Predicts replacing each of `merged_occurrence_ranges` with one symbol.
/// The buffers a candidate is tried on, reused from candidate to candidate so
/// predicting allocates only what the prediction keeps (MINE): a copy of the
/// stream's leaf counts, refreshed before every candidate; the two-queue
/// total's second queue; the symbol ids a change consumes or adds.
pub struct PredictionBuffers {
    leaf_counts: LeafCountsByWeight,
    merged_runs: MergedRunsBuffer,
    symbol_ids: Vec<u32>,
}

impl PredictionBuffers {
    pub fn new(stream: &SymbolStream) -> Self {
        PredictionBuffers {
            leaf_counts: stream.leaf_counts_by_weight().clone(),
            merged_runs: MergedRunsBuffer::default(),
            symbol_ids: Vec::new(),
        }
    }

    /// The stream's leaf counts with `changes` applied: (encoded symbol bits, leaves).
    fn total_after(&mut self, stream: &SymbolStream, changes: &[FrequencyChange]) -> (u64, usize) {
        self.leaf_counts.clone_from(stream.leaf_counts_by_weight());
        self.leaf_counts.apply_frequency_changes(changes, &stream.symbol_frequencies);
        (self.leaf_counts.encoded_symbol_bits_reusing(&mut self.merged_runs), self.leaf_counts.leaf_count())
    }
}

pub fn predict_merge(
    stream: &SymbolStream,
    prediction_buffers: &mut PredictionBuffers,
    candidate_index: usize,
    merged_definition: SymbolDefinition,
    merged_occurrence_ranges: Vec<OccurrenceRange>,
    code_lengths_before: &[u32],
) -> MergePrediction {
    let merged_symbol_id = stream.find_symbol_id(merged_definition).unwrap_or(stream.symbol_definitions.len());
    let consumed_symbol_ids = &mut prediction_buffers.symbol_ids;
    consumed_symbol_ids.clear();
    let mut consumed_code_bits_before = 0u64;
    for &(start_position, end_position) in &merged_occurrence_ranges {
        for position in start_position..end_position {
            let consumed_symbol_id = stream.placed_symbol_ids[position as usize];
            consumed_symbol_ids.push(consumed_symbol_id);
            consumed_code_bits_before += code_lengths_before[consumed_symbol_id as usize] as u64;
        }
    }
    // The merged symbol is longer than every part it consumes, so it is never one of them.
    let mut frequency_changes = Vec::new();
    count_into_frequency_changes(consumed_symbol_ids, -1, &mut frequency_changes);
    frequency_changes.push(FrequencyChange {
        symbol_id: merged_symbol_id as u32,
        delta: merged_occurrence_ranges.len() as i64,
    });
    frequency_changes.sort_unstable_by_key(|change| change.symbol_id);
    let (encoded_symbol_bits_after, leaves_after) = prediction_buffers.total_after(stream, &frequency_changes);
    MergePrediction {
        candidate_index,
        merged_definition,
        merged_symbol_id,
        merged_occurrence_ranges,
        frequency_changes,
        leaves_after,
        encoded_symbol_bits_after,
        consumed_code_bits_before,
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
        // Every symbol spelling the bits from start_bit on, shortest first.
        let first_bit_offset = definition.first_bit_offset + start_bit as u64;
        stream.for_each_symbol_starting_at(first_bit_offset, (bit_length - start_bit) as u32, |piece_bit_length, piece_symbol_id| {
            let (piece_bit_length, piece_symbol_id) = (piece_bit_length as usize, piece_symbol_id as usize);
            if piece_symbol_id == symbol_id || stream.symbol_frequencies[piece_symbol_id] == 0 {
                return;
            }
            if let Some(((code_bits_after, pieces_after), _)) = best_from_bit[start_bit + piece_bit_length] {
                let cost = (code_bits_after + code_lengths[piece_symbol_id] as u64, pieces_after + 1);
                if best_from_bit[start_bit].map_or(true, |(best_cost, _)| cost < best_cost) {
                    best_from_bit[start_bit] = Some((cost, piece_symbol_id as u32));
                }
            }
        });
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
    prediction_buffers: &mut PredictionBuffers,
    symbol_id: usize,
    code_lengths_before: &[u32],
) -> Option<LeafRemovalPrediction> {
    let replacement_symbol_ids = respell_with_fewest_code_bits(stream, symbol_id, code_lengths_before)?;
    let removed_frequency = stream.symbol_frequencies[symbol_id];
    let mut frequency_changes = vec![FrequencyChange { symbol_id: symbol_id as u32, delta: -(removed_frequency as i64) }];
    prediction_buffers.symbol_ids.clear();
    prediction_buffers.symbol_ids.extend_from_slice(&replacement_symbol_ids);
    count_into_frequency_changes(&mut prediction_buffers.symbol_ids, removed_frequency as i64, &mut frequency_changes);
    frequency_changes.sort_unstable_by_key(|change| change.symbol_id);
    let (encoded_symbol_bits_after, leaves_after) = prediction_buffers.total_after(stream, &frequency_changes);
    Some(LeafRemovalPrediction {
        removed_symbol_id: symbol_id,
        removed_frequency,
        replacement_symbol_ids,
        frequency_changes,
        leaves_after,
        encoded_symbol_bits_after,
        removed_code_bits_before: removed_frequency * code_lengths_before[symbol_id] as u64,
    })
}

/// Code bits after a change, for the log only: `occurrences` times the summed
/// code lengths of `symbol_ids` in a Huffman tree built for the frequencies
/// after `changes`. One heap build per call, so only an observer that asks
/// gets it (BuilderObserver::wants_code_lengths_after).
pub fn code_bits_after_changes(stream: &SymbolStream, changes: &[FrequencyChange], symbol_ids: &[u32], occurrences: u64) -> u64 {
    let mut frequencies_after = stream.symbol_frequencies.clone();
    for change in changes {
        let symbol_id = change.symbol_id as usize;
        if frequencies_after.len() <= symbol_id {
            frequencies_after.resize(symbol_id + 1, 0);
        }
        frequencies_after[symbol_id] = (frequencies_after[symbol_id] as i64 + change.delta) as u64;
    }
    let code_lengths_after = calculate_code_lengths(&frequencies_after);
    occurrences * symbol_ids.iter().map(|&symbol_id| code_lengths_after[symbol_id as usize] as u64).sum::<u64>()
}
