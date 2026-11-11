//! Decision: the bottom-up loop that builds the symbol dictionary.
//!
//! For each window size 2..=n, passes repeat until a pass commits nothing (or
//! an exact state repeats). A pass finds the merge candidates once, then runs
//! rounds: analyze every remaining candidate against the current state
//! (change_prediction), rank the options (option_ranking), and commit the
//! best option whose merge passes the commit rules (applied by symbol_stream).
//!
//! Commit rules:
//! * a merge must repeat: 2 or more non-overlapping occurrences;
//! * the merged symbol's code bits (tree after the merge) must be fewer than
//!   the code bits of the placements it replaces (tree before the merge).
//!
//! Options: a merge alone, or a merge plus the deletion of any subset of its
//! remaining parts (every subset is tried) when that lowers the leaf count.
//! A deleting option is committed even if it raises the encoded symbol bits:
//! a smaller dictionary is the goal, encoded symbol bits only break ties.

use crate::builder_events::*;
use crate::change_prediction::{
    calculate_encoded_symbol_bits_and_code_lengths, count_leaf_change, list_leaves_removed_and_added, predict_deletion,
    predict_merge,
};
use crate::merge_candidates::{find_merge_candidates, select_most_non_overlapping_occurrences, MergeCandidate};
use crate::option_ranking::{calculate_score_fraction, compare_by_rank, RankingKey};
use crate::predicted_changes::{MergeOption, MergePrediction};
use crate::symbol_definition::SymbolDefinition;
use crate::symbol_stream::{count_leaves, SymbolStream};
use std::cmp::Ordering;
use std::collections::HashSet;
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
    let mut summary = WindowSizeFinished {
        window_bit_length,
        passes: 0,
        merges_committed: 0,
        parts_deleted: 0,
        options_evaluated: 0,
        most_parts_in_one_candidate: 0,
        stopped_by_repeated_state: false,
        elapsed_milliseconds: 0,
    };
    // Deletions put pieces back, so merges and deletions could undo each other
    // in a loop of any length; stop if an exact state (all frequencies) repeats.
    let mut frequency_states_seen: HashSet<Vec<u64>> = HashSet::new();
    loop {
        summary.passes += 1;
        let merges_this_pass = run_pass(stream, window_bit_length, &mut summary, observer);
        summary.merges_committed += merges_this_pass;
        if merges_this_pass == 0 {
            break;
        }
        if !frequency_states_seen.insert(stream.symbol_frequencies.clone()) {
            summary.stopped_by_repeated_state = true;
            break;
        }
    }
    summary.elapsed_milliseconds = window_start.elapsed().as_millis();
    summary
}

enum RoundOutcome {
    NothingCommitted,
    MergeCommitted,
    MergeAndDeletionCommitted,
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
        let ranked_options = analyze_and_rank_options(
            stream,
            &merge_candidates,
            &context,
            &position_consumed_this_pass,
            &mut candidate_finished,
            summary,
            observer,
        );
        let outcome = commit_best_passing_option(
            stream,
            ranked_options,
            &context,
            &mut position_consumed_this_pass,
            &mut candidate_finished,
            summary,
            observer,
        );
        match outcome {
            RoundOutcome::NothingCommitted => break,
            RoundOutcome::MergeCommitted => merges_committed += 1,
            RoundOutcome::MergeAndDeletionCommitted => {
                // Positions changed; the candidates are stale. The next pass rescans.
                return merges_committed + 1;
            }
        }
    }
    stream.remove_vacated_positions();
    merges_committed
}

/// Analysis for one round: every remaining candidate becomes its best option,
/// and the options come back ranked best first.
fn analyze_and_rank_options(
    stream: &SymbolStream,
    merge_candidates: &[MergeCandidate],
    context: &RoundContext,
    position_consumed_this_pass: &[bool],
    candidate_finished: &mut [bool],
    summary: &mut WindowSizeFinished,
    observer: &mut dyn BuilderObserver,
) -> Vec<MergeOption> {
    let mut round = RoundAnalyzed {
        window_bit_length: context.window_bit_length,
        pass_number: context.pass_number,
        round_number: context.round_number,
        option_count: 0,
        best_options_with_deletion: 0,
        candidates_dropped_for_single_occurrence: 0,
        merges_that_shrink_alone: 0,
        deletion_subsets_tried: 0,
        deletion_subsets_unspellable: 0,
        deletion_subsets_not_shrinking: 0,
        deletion_subsets_shrinking: 0,
    };
    let mut options = Vec::new();
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
        options.push(find_best_option_for_merge(stream, merge, context, &mut round, summary));
    }
    options.sort_by(|first, second| {
        compare_by_rank(context.encoded_symbol_bits_before, &first.ranking_key(), &second.ranking_key())
    });
    round.option_count = options.len();
    round.best_options_with_deletion = options.iter().filter(|option| option.deletion.is_some()).count();
    observer.on_round_analyzed(&round, stream);
    options
}

/// The merge alone, or the merge plus deleting any subset of its remaining
/// parts that lowers the leaf count; whichever ranks highest.
fn find_best_option_for_merge(
    stream: &SymbolStream,
    merge: MergePrediction,
    context: &RoundContext,
    round: &mut RoundAnalyzed,
    summary: &mut WindowSizeFinished,
) -> MergeOption {
    let merge_alone_shrinks = count_leaves(&merge.symbol_frequencies_after) < context.leaves_before;
    let merge_alone_leaf_change =
        merge_alone_shrinks.then(|| count_leaf_change(&stream.symbol_frequencies, &merge.symbol_frequencies_after));
    round.merges_that_shrink_alone += merge_alone_shrinks as u64;

    let mut best_deletion = None;
    let mut best_leaf_change = merge_alone_leaf_change;
    let mut best_final_encoded_symbol_bits = merge.encoded_symbol_bits_after;

    let remaining_part_ids: Vec<usize> = merge
        .consumed_symbol_ids
        .iter()
        .copied()
        .filter(|&part_id| merge.symbol_frequencies_after[part_id] > 0)
        .collect();
    assert!(remaining_part_ids.len() < 64, "too many parts to enumerate their subsets");
    summary.most_parts_in_one_candidate = summary.most_parts_in_one_candidate.max(remaining_part_ids.len() as u32);
    let frequency = merge.merged_occurrence_ranges.len() as u64;

    for subset_mask in 1u64..(1u64 << remaining_part_ids.len()) {
        let deleted_part_ids: Vec<usize> = (0..remaining_part_ids.len())
            .filter(|&part_index| (subset_mask >> part_index) & 1 == 1)
            .map(|part_index| remaining_part_ids[part_index])
            .collect();
        round.deletion_subsets_tried += 1;
        let Some(deletion) = predict_deletion(stream, &merge, &deleted_part_ids) else {
            round.deletion_subsets_unspellable += 1;
            continue;
        };
        if count_leaves(&deletion.symbol_frequencies_after) >= context.leaves_before {
            round.deletion_subsets_not_shrinking += 1;
            continue;
        }
        round.deletion_subsets_shrinking += 1;
        let leaf_change = count_leaf_change(&stream.symbol_frequencies, &deletion.symbol_frequencies_after);
        let challenger = RankingKey {
            frequency,
            merged_bits: merge.merged_definition.bits,
            leaf_change_if_shrinking: Some(leaf_change),
            final_encoded_symbol_bits: deletion.encoded_symbol_bits_after,
        };
        let current_best = RankingKey {
            frequency,
            merged_bits: merge.merged_definition.bits,
            leaf_change_if_shrinking: best_leaf_change,
            final_encoded_symbol_bits: best_final_encoded_symbol_bits,
        };
        if compare_by_rank(context.encoded_symbol_bits_before, &challenger, &current_best) == Ordering::Less {
            best_final_encoded_symbol_bits = deletion.encoded_symbol_bits_after;
            best_leaf_change = Some(leaf_change);
            best_deletion = Some(deletion);
        }
    }
    MergeOption {
        merge,
        deletion: best_deletion,
        leaf_change_if_shrinking: best_leaf_change,
        final_encoded_symbol_bits: best_final_encoded_symbol_bits,
    }
}

/// Commit rule: the merged code must be shorter than the codes it replaces.
fn merged_code_is_shorter_than_replaced_codes(merge: &MergePrediction) -> bool {
    merge.merged_code_bits_after < merge.consumed_code_bits_before
}

/// Decision + application: commit the highest-ranked option whose merge
/// passes the commit rules.
fn commit_best_passing_option(
    stream: &mut SymbolStream,
    ranked_options: Vec<MergeOption>,
    context: &RoundContext,
    position_consumed_this_pass: &mut [bool],
    candidate_finished: &mut [bool],
    summary: &mut WindowSizeFinished,
    observer: &mut dyn BuilderObserver,
) -> RoundOutcome {
    for (rank_position, option) in ranked_options.into_iter().enumerate() {
        summary.options_evaluated += 1;
        candidate_finished[option.merge.candidate_index] = true;
        if !merged_code_is_shorter_than_replaced_codes(&option.merge) {
            let rejected = MergeRejected {
                window_bit_length: context.window_bit_length,
                pass_number: context.pass_number,
                round_number: context.round_number,
                rank_position,
                merged_definition: option.merge.merged_definition,
                frequency: option.merge.merged_occurrence_ranges.len(),
                reason: RejectionReason::MergedCodeNotShorter,
                merged_code_bits_after: option.merge.merged_code_bits_after,
                consumed_code_bits_before: option.merge.consumed_code_bits_before,
                symbols_its_deletion_would_remove: option.deletion.as_ref().map_or(0, |d| d.respellings.len()),
            };
            observer.on_merge_rejected(&rejected, stream);
            continue;
        }
        let committed = describe_change_before_applying(stream, &option, context, rank_position);
        stream.apply_merge(&option.merge, position_consumed_this_pass);
        if let Some(deletion) = &option.deletion {
            summary.parts_deleted += deletion.respellings.len() as u32;
            stream.apply_deletion(deletion);
            observer.on_change_committed(&committed, stream);
            return RoundOutcome::MergeAndDeletionCommitted;
        }
        observer.on_change_committed(&committed, stream);
        return RoundOutcome::MergeCommitted;
    }
    RoundOutcome::NothingCommitted
}

/// Builds the full record of a change from the state before it is applied.
fn describe_change_before_applying(
    stream: &SymbolStream,
    option: &MergeOption,
    context: &RoundContext,
    rank_position: usize,
) -> ChangeCommitted {
    let merge = &option.merge;
    let mut consumed_parts: Vec<ConsumedPart> = Vec::new();
    let mut consumed_placements_from_respelling = 0;
    for &(start_position, end_position) in &merge.merged_occurrence_ranges {
        for position in start_position..end_position {
            let consumed_symbol_id = stream.placed_symbol_ids[position as usize] as usize;
            consumed_placements_from_respelling += stream.placed_by_respelling[position as usize] as usize;
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
    let final_frequencies = option.deletion.as_ref().map_or(&merge.symbol_frequencies_after, |d| &d.symbol_frequencies_after);
    let (removed_leaf_ids, added_leaf_ids) = list_leaves_removed_and_added(&stream.symbol_frequencies, final_frequencies);
    let symbol_count_before = stream.symbol_definitions.len();
    let revived_symbol_count = added_leaf_ids.iter().filter(|&&symbol_id| symbol_id < symbol_count_before).count();
    let respellings = option
        .deletion
        .as_ref()
        .map(|deletion| {
            deletion
                .respellings
                .iter()
                .map(|respelling| RespellingRecord {
                    deleted_symbol_id: respelling.deleted_symbol_id,
                    deleted_frequency: merge.symbol_frequencies_after[respelling.deleted_symbol_id],
                    replacement_symbol_ids: respelling.replacement_symbol_ids.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let (score_numerator, score_denominator) = calculate_score_fraction(context.encoded_symbol_bits_before, &option.ranking_key());
    let removed_count = removed_leaf_ids.len();
    let added_count = added_leaf_ids.len();
    ChangeCommitted {
        window_bit_length: context.window_bit_length,
        pass_number: context.pass_number,
        round_number: context.round_number,
        rank_position,
        kind: if option.deletion.is_some() {
            CommitKind::MergeWithDeletion
        } else if removed_count > added_count {
            CommitKind::MergeThatShrinksAlone
        } else {
            CommitKind::Merge
        },
        merged_definition: merge.merged_definition,
        merged_symbol_is_new: merge.merged_symbol_id == symbol_count_before,
        frequency: merge.merged_occurrence_ranges.len(),
        consumed_parts,
        merged_code_bits_after: merge.merged_code_bits_after,
        consumed_code_bits_before: merge.consumed_code_bits_before,
        score_numerator,
        score_denominator,
        encoded_symbol_bits_before: context.encoded_symbol_bits_before,
        encoded_symbol_bits_after_merge: merge.encoded_symbol_bits_after,
        encoded_symbol_bits_final: option.final_encoded_symbol_bits,
        leaves_before: context.leaves_before,
        removed_leaf_ids,
        added_leaf_ids,
        revived_symbol_count,
        respellings,
        placed_symbols_before: stream.occupied_position_count(),
        consumed_placements_from_respelling,
    }
}
