//! Plain data describing predicted changes. Produced by the analysis stage
//! (change_prediction), ranked by option_ranking, chosen by symbol_builder,
//! applied by symbol_stream. No logic lives here.

use crate::symbol_definition::SymbolDefinition;

/// One symbol's frequency change, computed from the candidate's own data
/// (the user's design, 2026-11-11): a change is tried and committed as its
/// list of these, sorted by symbol id, never as a whole frequency table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrequencyChange {
    pub symbol_id: u32,
    pub delta: i64,
}

/// A range of positions in the placed-symbol sequence: start, end (exclusive).
pub type OccurrenceRange = (u32, u32);

/// What merging one candidate into a single symbol would do.
pub struct MergePrediction {
    pub candidate_index: usize,
    pub merged_definition: SymbolDefinition,
    /// Equals the current symbol count when the merged symbol is new.
    pub merged_symbol_id: usize,
    pub merged_occurrence_ranges: Vec<OccurrenceRange>,
    pub frequency_changes: Vec<FrequencyChange>,
    pub leaves_after: usize,
    pub encoded_symbol_bits_after: u64,
    /// Code bits of every consumed placement, in the tree before the merge.
    pub consumed_code_bits_before: u64,
}

/// What removing one leaf would do: every placement of it is replaced by the
/// same respelling made of other live leaves.
pub struct LeafRemovalPrediction {
    pub removed_symbol_id: usize,
    pub removed_frequency: u64,
    pub replacement_symbol_ids: Vec<u32>,
    pub frequency_changes: Vec<FrequencyChange>,
    pub leaves_after: usize,
    pub encoded_symbol_bits_after: u64,
    /// Frequency * code length of the removed leaf, in the tree before.
    pub removed_code_bits_before: u64,
}

/// The frequency `symbol_id` has after `changes`, given its frequency before.
pub fn frequency_after_changes(changes: &[FrequencyChange], symbol_id: usize, frequency_before: u64) -> u64 {
    let delta: i64 = changes.iter().filter(|change| change.symbol_id as usize == symbol_id).map(|change| change.delta).sum();
    (frequency_before as i64 + delta) as u64
}

/// Turns symbol ids (with repeats) into frequency changes of `delta_per_occurrence`
/// each, one per symbol, sorted by symbol id.
pub fn count_into_frequency_changes(symbol_ids: &mut [u32], delta_per_occurrence: i64, changes: &mut Vec<FrequencyChange>) {
    symbol_ids.sort_unstable();
    for run in symbol_ids.chunk_by(|first, second| first == second) {
        changes.push(FrequencyChange { symbol_id: run[0], delta: delta_per_occurrence * run.len() as i64 });
    }
}
