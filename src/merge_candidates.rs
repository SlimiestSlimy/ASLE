//! Analysis: finding merge candidates for one window size.
//!
//! A merge candidate is a bit string of exactly the window size that is
//! filled by 2 or more adjacent placed symbols, nothing poking out of the
//! window. Raw bits are placed "0"/"1" symbols, so this covers both scanning
//! uncovered bits and merging placed symbols (and any mix of the two).

use crate::fast_hash::FastHashMap;
use crate::predicted_changes::OccurrenceRange;
use crate::symbol_definition::SymbolDefinition;
use crate::symbol_stream::SymbolStream;

pub struct MergeCandidate {
    /// Points at the candidate's first occurrence in the input bits.
    pub definition: SymbolDefinition,
    /// Every place the window is filled exactly, overlapping ones included,
    /// in increasing order of position.
    pub all_occurrence_ranges: Vec<OccurrenceRange>,
}

pub fn find_merge_candidates(stream: &SymbolStream, window_bit_length: u32) -> Vec<MergeCandidate> {
    let mut candidate_indexes_by_content_hash: FastHashMap<u64, Vec<usize>> = FastHashMap::default();
    let mut merge_candidates: Vec<MergeCandidate> = Vec::new();
    let placed_count = stream.placed_symbol_ids.len();
    let mut start_bit_offset = 0u64;
    for start_position in 0..placed_count {
        let mut filled_bit_length = 0u32;
        let mut end_position = start_position;
        while end_position < placed_count && filled_bit_length < window_bit_length {
            let bit_length = stream.symbol_definitions[stream.placed_symbol_ids[end_position] as usize].bit_length;
            if filled_bit_length + bit_length > window_bit_length {
                filled_bit_length += bit_length;
                break;
            }
            filled_bit_length += bit_length;
            end_position += 1;
        }
        let window_filled_exactly = filled_bit_length == window_bit_length;
        let made_of_several_symbols = end_position - start_position >= 2;
        if window_filled_exactly && made_of_several_symbols {
            let window = SymbolDefinition { first_bit_offset: start_bit_offset, bit_length: window_bit_length };
            let content_hash = stream.hash_definition(window);
            let same_hash_indexes = candidate_indexes_by_content_hash.entry(content_hash).or_default();
            let existing_index = same_hash_indexes
                .iter()
                .copied()
                .find(|&index| stream.definitions_have_equal_bits(merge_candidates[index].definition, window));
            let candidate_index = match existing_index {
                Some(index) => index,
                None => {
                    merge_candidates.push(MergeCandidate { definition: window, all_occurrence_ranges: Vec::new() });
                    same_hash_indexes.push(merge_candidates.len() - 1);
                    merge_candidates.len() - 1
                }
            };
            merge_candidates[candidate_index].all_occurrence_ranges.push((start_position as u32, end_position as u32));
        }
        start_bit_offset += stream.symbol_definitions[stream.placed_symbol_ids[start_position] as usize].bit_length as u64;
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
