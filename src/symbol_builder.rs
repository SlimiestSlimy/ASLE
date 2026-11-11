//! Decision: the bottom-up loop that builds the symbol dictionary.
//!
//! For each window size 2..=n, passes repeat until a pass commits nothing.
//! A pass finds the merge candidates once, then runs rounds: predict every
//! remaining candidate's merge against the current state (change_prediction),
//! rank the merges (option_ranking), and commit the best merge that passes the
//! commit rules (applied by symbol_stream). Every merge strictly reduces the
//! number of placed symbols, so this always terminates.
//!
//! Commit rules:
//! * a merge must repeat: 2 or more non-overlapping occurrences;
//! * the merged symbol's code bits (tree after the merge) must be fewer than
//!   the code bits of the placements it replaces (tree before the merge).

use crate::builder_events::*;
use crate::change_prediction::{
    calculate_encoded_symbol_bits_and_code_lengths, list_leaves_removed_and_added, predict_merge,
};
use crate::merge_candidates::{find_merge_candidates, select_most_non_overlapping_occurrences, MergeCandidate};
use crate::option_ranking::{calculate_score_numerator, compare_by_rank};
use crate::predicted_changes::MergePrediction;
use crate::symbol_definition::SymbolDefinition;
use crate::symbol_stream::{count_leaves, SymbolStream};
use std::time::Instant;

pub fn build_symbol_dictionary(stream: &mut SymbolStream, observer: &mut dyn BuilderObserver) {
    let build_start = Instant::now();
    observer.on_build_started(stream);
    for window_bit_length in 2..=stream.maximum_symbol_bit_length {
        observer.on_window_size_started(window_bit_length, stream);
        let summary = run_window_size(stream, window_bit_length, observer);
        observer.on_window_size_finished(&summary, stream);
    }
    let finished = BuildFinished {
        symbols_created: stream.symbol_definitions.len(),
        elapsed_milliseconds: build_start.elapsed().as_millis(),
    };
    observer.on_build_finished(&finished, stream);
}

fn run_window_size(
    stream: &mut SymbolStream,
    window_bit_length: u32,
    observer: &mut dyn BuilderObserver,
) -> WindowSizeFinished {
    let window_start = Instant::now();
    let mut summary =
        WindowSizeFinished { window_bit_length, passes: 0, merges_committed: 0, merges_evaluated: 0, elapsed_milliseconds: 0 };
    loop {
        summary.passes += 1;
        let merges_this_pass = run_pass(stream, window_bit_length, &mut summary, observer);
        summary.merges_committed += merges_this_pass;
        if merges_this_pass == 0 {
            break;
        }
    }
    summary.elapsed_milliseconds = window_start.elapsed().as_millis();
    summary
}

/// Shared facts about the state at the start of a round.
struct RoundContext {
    window_bit_length: u32,
    pass_number: u32,
    round_number: u32,
    encoded_symbol_bits_before: u64,
    code_lengths_before: Vec<u32>,
    leaves_before: usize,
}

/// Returns the number of merges committed in this pass.
fn run_pass(
    stream: &mut SymbolStream,
    window_bit_length: u32,
    summary: &mut WindowSizeFinished,
    observer: &mut dyn BuilderObserver,
) -> u32 {
    let merge_candidates = find_merge_candidates(stream, window_bit_length);
    let pass_started = PassStarted {
        window_bit_length,
        pass_number: summary.passes,
        candidate_count: merge_candidates.len(),
        occurrence_count: merge_candidates.iter().map(|candidate| candidate.all_occurrence_ranges.len()).sum(),
    };
    observer.on_pass_started(&pass_started, stream);
    let mut position_consumed_this_pass = vec![false; stream.placed_symbol_ids.len()];
    let mut candidate_finished = vec![false; merge_candidates.len()];
    let mut merges_committed = 0;
    let mut round_number = 0;
    loop {
        round_number += 1;
        let (encoded_symbol_bits_before, code_lengths_before) =
            calculate_encoded_symbol_bits_and_code_lengths(&stream.symbol_frequencies);
        let context = RoundContext {
            window_bit_length,
            pass_number: summary.passes,
            round_number,
            encoded_symbol_bits_before,
            code_lengths_before,
            leaves_before: stream.leaf_count(),
        };
        let ranked_merges = predict_and_rank_merges(
            stream,
            &merge_candidates,
            &context,
            &position_consumed_this_pass,
            &mut candidate_finished,
            observer,
        );
        let committed = commit_best_passing_merge(
            stream,
            ranked_merges,
            &context,
            &mut position_consumed_this_pass,
            &mut candidate_finished,
            summary,
            observer,
        );
        if !committed {
            break;
        }
        merges_committed += 1;
    }
    stream.remove_vacated_positions();
    merges_committed
}

/// Analysis for one round: every remaining candidate's merge is predicted,
/// and the merges come back ranked best first.
fn predict_and_rank_merges(
    stream: &SymbolStream,
    merge_candidates: &[MergeCandidate],
    context: &RoundContext,
    position_consumed_this_pass: &[bool],
    candidate_finished: &mut [bool],
    observer: &mut dyn BuilderObserver,
) -> Vec<MergePrediction> {
    let mut round = RoundAnalyzed {
        window_bit_length: context.window_bit_length,
        pass_number: context.pass_number,
        round_number: context.round_number,
        merges_ranked: 0,
        candidates_dropped_for_single_occurrence: 0,
        merges_that_lower_the_leaf_count: 0,
    };
    let mut merges = Vec::new();
    for (candidate_index, candidate) in merge_candidates.iter().enumerate() {
        if candidate_finished[candidate_index] {
            continue;
        }
        let occurrence_ranges =
            select_most_non_overlapping_occurrences(&candidate.all_occurrence_ranges, position_consumed_this_pass);
        // Commit rule: a merge must repeat.
        if occurrence_ranges.len() < 2 {
            candidate_finished[candidate_index] = true;
            round.candidates_dropped_for_single_occurrence += 1;
            continue;
        }
        let merged_definition = SymbolDefinition { bits: candidate.bits, bit_length: context.window_bit_length };
        let merge = predict_merge(stream, candidate_index, merged_definition, occurrence_ranges, &context.code_lengths_before);
        round.merges_that_lower_the_leaf_count +=
            (count_leaves(&merge.symbol_frequencies_after) < context.leaves_before) as u64;
        merges.push(merge);
    }
    merges.sort_by(|first, second| {
        compare_by_rank(context.encoded_symbol_bits_before, &first.ranking_key(), &second.ranking_key())
    });
    round.merges_ranked = merges.len();
    observer.on_round_analyzed(&round, stream);
    merges
}

/// Commit rule: the merged code must be shorter than the codes it replaces.
fn merged_code_is_shorter_than_replaced_codes(merge: &MergePrediction) -> bool {
    merge.merged_code_bits_after < merge.consumed_code_bits_before
}

/// Decision + application: commit the highest-ranked merge that passes the
/// commit rules. Returns whether one was committed.
fn commit_best_passing_merge(
    stream: &mut SymbolStream,
    ranked_merges: Vec<MergePrediction>,
    context: &RoundContext,
    position_consumed_this_pass: &mut [bool],
    candidate_finished: &mut [bool],
    summary: &mut WindowSizeFinished,
    observer: &mut dyn BuilderObserver,
) -> bool {
    for (rank_position, merge) in ranked_merges.into_iter().enumerate() {
        summary.merges_evaluated += 1;
        candidate_finished[merge.candidate_index] = true;
        if !merged_code_is_shorter_than_replaced_codes(&merge) {
            let rejected = MergeRejected {
                window_bit_length: context.window_bit_length,
                pass_number: context.pass_number,
                round_number: context.round_number,
                rank_position,
                merged_definition: merge.merged_definition,
                frequency: merge.merged_occurrence_ranges.len(),
                reason: RejectionReason::MergedCodeNotShorter,
                merged_code_bits_after: merge.merged_code_bits_after,
                consumed_code_bits_before: merge.consumed_code_bits_before,
            };
            observer.on_merge_rejected(&rejected, stream);
            continue;
        }
        let committed = describe_merge_before_applying(stream, &merge, context, rank_position);
        stream.apply_merge(&merge, position_consumed_this_pass);
        observer.on_change_committed(&committed, stream);
        return true;
    }
    false
}

/// Builds the full record of a merge from the state before it is applied.
fn describe_merge_before_applying(
    stream: &SymbolStream,
    merge: &MergePrediction,
    context: &RoundContext,
    rank_position: usize,
) -> ChangeCommitted {
    let mut consumed_parts: Vec<ConsumedPart> = Vec::new();
    for &(start_position, end_position) in &merge.merged_occurrence_ranges {
        for position in start_position..end_position {
            let consumed_symbol_id = stream.placed_symbol_ids[position as usize] as usize;
            match consumed_parts.iter_mut().find(|part| part.symbol_id == consumed_symbol_id) {
                Some(part) => part.consumed_count += 1,
                None => consumed_parts.push(ConsumedPart {
                    symbol_id: consumed_symbol_id,
                    consumed_count: 1,
                    frequency_after_merge: merge.symbol_frequencies_after[consumed_symbol_id],
                }),
            }
        }
    }
    let (removed_leaf_ids, added_leaf_ids) =
        list_leaves_removed_and_added(&stream.symbol_frequencies, &merge.symbol_frequencies_after);
    ChangeCommitted {
        window_bit_length: context.window_bit_length,
        pass_number: context.pass_number,
        round_number: context.round_number,
        rank_position,
        kind: if removed_leaf_ids.len() > added_leaf_ids.len() {
            CommitKind::MergeThatLowersTheLeafCount
        } else {
            CommitKind::Merge
        },
        merged_definition: merge.merged_definition,
        merged_symbol_is_new: merge.merged_symbol_id == stream.symbol_definitions.len(),
        frequency: merge.merged_occurrence_ranges.len(),
        consumed_parts,
        merged_code_bits_after: merge.merged_code_bits_after,
        consumed_code_bits_before: merge.consumed_code_bits_before,
        score_numerator: calculate_score_numerator(context.encoded_symbol_bits_before, &merge.ranking_key()),
        encoded_symbol_bits_before: context.encoded_symbol_bits_before,
        encoded_symbol_bits_after: merge.encoded_symbol_bits_after,
        leaves_before: context.leaves_before,
        removed_leaf_ids,
        added_leaf_ids,
        placed_symbols_before: stream.occupied_position_count(),
    }
}
