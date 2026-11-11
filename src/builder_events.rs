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
    pub option_count: usize,
    pub best_options_with_deletion: usize,
    pub candidates_dropped_for_single_occurrence: u64,
    pub merges_that_shrink_alone: u64,
    pub deletion_subsets_tried: u64,
    pub deletion_subsets_unspellable: u64,
    pub deletion_subsets_not_shrinking: u64,
    pub deletion_subsets_shrinking: u64,
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
    pub symbols_its_deletion_would_remove: usize,
}

pub enum CommitKind {
    Merge,
    MergeThatShrinksAlone,
    MergeWithDeletion,
}

pub struct ConsumedPart {
    pub symbol_id: usize,
    pub consumed_count: u64,
    pub frequency_after_merge: u64,
}

pub struct RespellingRecord {
    pub deleted_symbol_id: usize,
    pub deleted_frequency: u64,
    pub replacement_symbol_ids: Vec<u32>,
}

/// Everything about one committed change. Symbol ids refer to the stream as
/// it is after the change (ids are never reused, so they stay valid).
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
    pub score_denominator: i128,
    pub huffman_weight_before: u64,
    pub huffman_weight_after_merge: u64,
    pub huffman_weight_final: u64,
    pub leaves_before: usize,
    pub removed_leaf_ids: Vec<usize>,
    pub added_leaf_ids: Vec<usize>,
    pub revived_symbol_count: usize,
    pub respellings: Vec<RespellingRecord>,
    pub placed_symbols_before: usize,
    pub consumed_placements_from_respelling: usize,
}

pub struct WindowSizeFinished {
    pub window_bit_length: u32,
    pub passes: u32,
    pub merges_committed: u32,
    pub parts_deleted: u32,
    pub options_evaluated: u64,
    pub most_parts_in_one_candidate: u32,
    pub stopped_by_repeated_state: bool,
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
