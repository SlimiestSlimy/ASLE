//! Analysis: finding merge candidates for one window size.
//!
//! A merge candidate is a bit string of exactly the window size that is
//! filled by 2 or more adjacent placed symbols, nothing poking out of the
//! window. Raw bits are placed "0"/"1" symbols, so this covers both scanning
//! uncovered bits and merging placed symbols (and any mix of the two).

use crate::fast_hash::FastHashMap;
use crate::predicted_changes::OccurrenceRange;
use crate::symbol_stream::SymbolStream;

pub struct MergeCandidate {
    pub bits: u64,
    /// Every place the window is filled exactly, overlapping ones included,
    /// in increasing order of position.
    pub all_occurrence_ranges: Vec<OccurrenceRange>,
}

pub fn find_merge_candidates(stream: &SymbolStream, window_bit_length: u32) -> Vec<MergeCandidate> {
    let mut candidate_index_by_bits: FastHashMap<u64, usize> = FastHashMap::default();
    let mut merge_candidates: Vec<MergeCandidate> = Vec::new();
    let placed_count = stream.placed_symbol_ids.len();
    for start_position in 0..placed_count {
        let mut window_bits = 0u64;
        let mut filled_bit_length = 0u32;
        let mut end_position = start_position;
        while end_position < placed_count && filled_bit_length < window_bit_length {
            let definition = stream.symbol_definitions[stream.placed_symbol_ids[end_position] as usize];
            filled_bit_length += definition.bit_length;
            if filled_bit_length > window_bit_length {
                break;
            }
            window_bits = (window_bits << definition.bit_length) | definition.bits;
            end_position += 1;
        }
        let window_filled_exactly = filled_bit_length == window_bit_length;
        let made_of_several_symbols = end_position - start_position >= 2;
        if window_filled_exactly && made_of_several_symbols {
            let candidate_index = *candidate_index_by_bits.entry(window_bits).or_insert_with(|| {
                merge_candidates.push(MergeCandidate { bits: window_bits, all_occurrence_ranges: Vec::new() });
                merge_candidates.len() - 1
            });
            merge_candidates[candidate_index].all_occurrence_ranges.push((start_position as u32, end_position as u32));
        }
    }
    merge_candidates
}

/// The most non-overlapping occurrences among those still intact (no
/// position consumed this pass). All occurrences of a candidate have the
/// same bit length, so taking them left to right gives the maximum.
pub fn select_most_non_overlapping_occurrences(
    all_occurrence_ranges: &[OccurrenceRange],
    position_consumed_this_pass: &[bool],
) -> Vec<OccurrenceRange> {
    let mut selected_ranges = Vec::new();
    let mut end_of_last_selected = 0u32;
    for &(start_position, end_position) in all_occurrence_ranges {
        let overlaps_last_selected = start_position < end_of_last_selected;
        let touches_consumed_position =
            (start_position..end_position).any(|position| position_consumed_this_pass[position as usize]);
        if !overlaps_last_selected && !touches_consumed_position {
            selected_ranges.push((start_position, end_position));
            end_of_last_selected = end_position;
        }
    }
    selected_ranges
}
