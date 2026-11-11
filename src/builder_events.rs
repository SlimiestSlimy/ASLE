//! What the symbol builder reports while it runs. The builder only fills in
//! these records and hands them to an observer; how they are shown or stored
//! (terminal, JSON Lines file, nothing) is the observer's business.

use crate::symbol_definition::SymbolDefinition;
use crate::symbol_stream::SymbolStream;

pub struct PassStarted {
    pub window_bit_length: u32,
    pub pass_number: u32,
    pub candidate_count: usize,
    pub occurrence_count: usize,
}

pub struct RoundAnalyzed {
    pub window_bit_length: u32,
    pub pass_number: u32,
    pub round_number: u32,
    pub merges_ranked: usize,
    /// Leaves whose code is longer than their bits, found at round start.
    pub non_saving_leaves: usize,
    /// Non-saving leaves that could not be respelled with other live leaves.
    pub non_saving_leaves_unspellable: usize,
    pub candidates_dropped_for_single_occurrence: u64,
    pub merges_that_lower_the_leaf_count: u64,
}

pub enum RejectionReason {
    /// The total encoded symbol bits after the merge would not be lower.
    EncodedSymbolBitsDoNotDrop,
}

pub struct MergeRejected {
    pub window_bit_length: u32,
    pub pass_number: u32,
    pub round_number: u32,
    pub rank_position: usize,
    pub merged_definition: SymbolDefinition,
    pub frequency: usize,
    pub reason: RejectionReason,
    pub merged_code_bits_after: u64,
    pub consumed_code_bits_before: u64,
}

pub enum CommitKind {
    Merge,
    MergeThatLowersTheLeafCount,
}

pub struct ConsumedPart {
    pub symbol_id: usize,
    pub consumed_count: u64,
    pub frequency_after_merge: u64,
}

/// Everything about one committed merge. Symbol ids refer to the stream as
/// it is after the merge (ids are never reused, so they stay valid).
pub struct ChangeCommitted {
    pub window_bit_length: u32,
    pub pass_number: u32,
    pub round_number: u32,
    pub rank_position: usize,
    pub kind: CommitKind,
    pub merged_definition: SymbolDefinition,
    pub merged_symbol_is_new: bool,
    /// False when no merge lowered the total and this one was committed anyway
    /// (best point: every window size commits at least one merge).
    pub lowered_the_total: bool,
    pub frequency: usize,
    pub consumed_parts: Vec<ConsumedPart>,
    pub merged_code_bits_after: u64,
    pub consumed_code_bits_before: u64,
    pub score_numerator: i128,
    pub encoded_symbol_bits_before: u64,
    pub encoded_symbol_bits_after: u64,
    pub leaves_before: usize,
    pub removed_leaf_ids: Vec<usize>,
    pub added_leaf_ids: Vec<usize>,
    pub placed_symbols_before: usize,
}

/// Everything about one removed leaf. Symbol ids refer to the stream after.
pub struct LeafRemoved {
    pub window_bit_length: u32,
    pub pass_number: u32,
    pub round_number: u32,
    pub removed_symbol_id: usize,
    pub removed_frequency: u64,
    pub removed_bit_length: u32,
    pub removed_code_length_before: u32,
    pub removed_code_bits_before: u64,
    pub replacement_symbol_ids: Vec<u32>,
    pub replacement_code_bits_after: u64,
    pub encoded_symbol_bits_before: u64,
    pub encoded_symbol_bits_after: u64,
    pub leaves_before: usize,
    pub non_saving_leaves: usize,
    /// False when the removal was forced or removed a leaf placed once.
    pub lowered_the_total: bool,
    /// Removed automatically: the leaf was placed only once.
    pub removed_because_placed_once: bool,
}

pub struct WindowSizeFinished {
    pub window_bit_length: u32,
    pub passes: u32,
    pub merges_committed: u32,
    /// The window size's best-ranked merge was committed without lowering the total.
    pub merge_committed_without_lowering_the_total: bool,
    /// The window size's forced change was a leaf removal.
    pub leaf_removed_without_lowering_the_total: bool,
    pub leaves_removed: u32,
    /// Of leaves_removed, those removed automatically for being placed once.
    pub single_placement_leaves_removed: u32,
    pub stopped_by_repeated_state: bool,
    pub merges_evaluated: u64,
    pub elapsed_milliseconds: u128,
}

pub struct BuildFinished {
    pub symbols_created: usize,
    pub elapsed_milliseconds: u128,
}

/// Every method has an empty default, so an observer implements only what it needs.
pub trait BuilderObserver {
    /// Whether events should carry code lengths after a change
    /// (merged_code_bits_after, replacement_code_bits_after). They take a
    /// Huffman build per event, so they are computed only for an observer
    /// that asks (the JSON lines log); otherwise they are 0.
    fn wants_code_lengths_after(&self) -> bool {
        false
    }
    fn on_build_started(&mut self, _stream: &SymbolStream) {}
    fn on_window_size_started(&mut self, _window_bit_length: u32, _stream: &SymbolStream) {}
    fn on_pass_started(&mut self, _event: &PassStarted, _stream: &SymbolStream) {}
    fn on_round_analyzed(&mut self, _event: &RoundAnalyzed, _stream: &SymbolStream) {}
    fn on_merge_rejected(&mut self, _event: &MergeRejected, _stream: &SymbolStream) {}
    fn on_change_committed(&mut self, _event: &ChangeCommitted, _stream_after: &SymbolStream) {}
    fn on_leaf_removed(&mut self, _event: &LeafRemoved, _stream_after: &SymbolStream) {}
    fn on_window_size_finished(&mut self, _event: &WindowSizeFinished, _stream: &SymbolStream) {}
    fn on_build_finished(&mut self, _event: &BuildFinished, _stream: &SymbolStream) {}
}
