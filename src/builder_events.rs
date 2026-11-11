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
    pub candidates_dropped_for_single_occurrence: u64,
    pub merges_that_lower_the_leaf_count: u64,
}

pub enum RejectionReason {
    /// The merged symbol's code bits (tree after) are not fewer than the code
    /// bits of the placements it replaces (tree before).
    MergedCodeNotShorter,
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

pub struct WindowSizeFinished {
    pub window_bit_length: u32,
    pub passes: u32,
    pub merges_committed: u32,
    pub merges_evaluated: u64,
    pub elapsed_milliseconds: u128,
}

pub struct BuildFinished {
    pub symbols_created: usize,
    pub elapsed_milliseconds: u128,
}

/// Every method has an empty default, so an observer implements only what it needs.
pub trait BuilderObserver {
    fn on_build_started(&mut self, _stream: &SymbolStream) {}
    fn on_window_size_started(&mut self, _window_bit_length: u32, _stream: &SymbolStream) {}
    fn on_pass_started(&mut self, _event: &PassStarted, _stream: &SymbolStream) {}
    fn on_round_analyzed(&mut self, _event: &RoundAnalyzed, _stream: &SymbolStream) {}
    fn on_merge_rejected(&mut self, _event: &MergeRejected, _stream: &SymbolStream) {}
    fn on_change_committed(&mut self, _event: &ChangeCommitted, _stream_after: &SymbolStream) {}
    fn on_window_size_finished(&mut self, _event: &WindowSizeFinished, _stream: &SymbolStream) {}
    fn on_build_finished(&mut self, _event: &BuildFinished, _stream: &SymbolStream) {}
}
