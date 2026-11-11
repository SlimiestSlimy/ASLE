//! Plain data describing a predicted merge. Produced by the analysis stage
//! (change_prediction), ranked by option_ranking, chosen by symbol_builder,
//! applied by symbol_stream. No logic lives here.

use crate::symbol_definition::SymbolDefinition;

/// A range of positions in the placed-symbol sequence: start, end (exclusive).
pub type OccurrenceRange = (u32, u32);

/// What merging one candidate into a single symbol would do.
pub struct MergePrediction {
    pub candidate_index: usize,
    pub merged_definition: SymbolDefinition,
    /// Equals the current symbol count when the merged symbol is new.
    pub merged_symbol_id: usize,
    pub merged_occurrence_ranges: Vec<OccurrenceRange>,
    pub symbol_frequencies_after: Vec<u64>,
    pub encoded_symbol_bits_after: u64,
    /// Code bits of every consumed placement, in the tree before the merge.
    pub consumed_code_bits_before: u64,
    /// Occurrences * code length of the merged symbol, in the tree after the merge.
    pub merged_code_bits_after: u64,
}
