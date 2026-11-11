//! Plain data describing predicted changes to the symbol stream. Produced by
//! the analysis stage (change_prediction), ranked by option_ranking, chosen by
//! symbol_builder, applied by symbol_stream. No logic lives here.

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
    /// Encoded symbol bits after the merge.
    pub encoded_symbol_bits_after: u64,
    /// Distinct symbols the merge consumes, in order of first appearance.
    pub consumed_symbol_ids: Vec<usize>,
    /// Code bits of every consumed placement, in the tree before the merge.
    pub consumed_code_bits_before: u64,
    /// Occurrences * code length of the merged symbol, in the tree after the merge.
    pub merged_code_bits_after: u64,
}

/// One deleted symbol and the existing (or revived) symbols that spell its bits.
pub struct Respelling {
    pub deleted_symbol_id: usize,
    pub replacement_symbol_ids: Vec<u32>,
}

/// What deleting some of a merge's remaining parts would do, after that merge.
pub struct DeletionPrediction {
    pub respellings: Vec<Respelling>,
    pub symbol_frequencies_after: Vec<u64>,
    /// Encoded symbol bits after the merge and the deletion.
    pub encoded_symbol_bits_after: u64,
}

#[derive(Clone, Copy)]
pub struct LeafChange {
    pub leaves_removed: u64,
    pub leaves_added: u64,
}

/// A merge, optionally with a deletion, as one rankable option.
pub struct MergeOption {
    pub merge: MergePrediction,
    pub deletion: Option<DeletionPrediction>,
    /// Present only when the option lowers the leaf count.
    pub leaf_change_if_shrinking: Option<LeafChange>,
    /// Encoded symbol bits after everything the option does.
    pub final_encoded_symbol_bits: u64,
}
