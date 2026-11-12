//! Decision: the bottom-up loop that builds the symbol dictionary.
//!
//! For each window size 2, 3, 5, 8, 13, ... (Fibonacci, up to n), passes repeat until a pass commits nothing (or
//! an exact state repeats). A pass finds the merge candidates once, then runs
//! rounds. Each round, against the current state:
//! 0. A leaf placed only once is removed automatically, respelled with other
//!    live leaves (fewest code bits), whether or not the total drops (the
//!    user's rule, 2026-11-11): it cannot save anything in the body. Several:
//!    lowest symbol id first (MINE). Positions change, so the pass ends.
//! 1. Leaf removal first: every leaf whose code is longer than its own bits
//!    (non-saving; break-even leaves stay), biggest loss first, is predicted respelled with other
//!    live leaves (fewest code bits); the first that can be respelled and is
//!    worth removing is removed. Positions change, so the pass ends.
//! 2. Otherwise merges: predict every remaining candidate's merge, rank them
//!    (option_ranking), commit the best one that passes the merge rules.
//!
//! Commit rules, judged on the whole: a change (merge or leaf removal) is
//! committed when it strictly lowers the total encoded symbol bits (sum over
//! all leaves of frequency * Huffman code length). Header bits never enter a
//! decision. A merge must also repeat: 2 or more non-overlapping occurrences.
//!
//! Best point: once per window size, a change is forced. If a window size
//! reaches a round where no change lowers the total, and it has neither
//! committed a merge nor forced a change yet, the best change of the whole pool
//! is committed anyway: every ranked merge and every respellable non-saving
//! leaf's removal (the user's rule, 2026-11-11). Best, as everywhere else, is
//! the lowest total encoded symbol bits after the change; ties go to fewer
//! leaves after, then to the removal, then to merge rank order (tie-breaks
//! MINE). After that the normal rule applies again. This gets the build past
//! points where only several merges together pay off, and lets it remove a
//! leaf there instead of adding one.
//!
//! A removal puts placements back, so a merge and a removal could undo each
//! other; a window size stops when an exact state (all frequencies) repeats.

use crate::builder_events::*;
use crate::change_prediction::{
    code_bits_after_changes, PredictionBuffers, find_non_saving_leaves, list_leaves_removed_and_added,
    predict_leaf_removal, predict_merge,
};
use crate::merge_candidates::{find_merge_candidates, select_most_non_overlapping_occurrences, MergeCandidate};
use crate::option_ranking::{calculate_score_numerator, compare_by_rank};
use crate::predicted_changes::{frequency_after_changes, LeafRemovalPrediction, MergePrediction};
use crate::symbol_stream::SymbolStream;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::time::Instant;

/// Window sizes follow the Fibonacci sequence, 2, 3, 5, 8, 13, 21, ..., up to
/// the largest one not above n (the user's design, 2026-11-11): far fewer
/// window sizes, and each is the sum of the two before it, so one symbol of
/// each of the two previous sizes fills a window exactly.
fn fibonacci_window_sizes(minimum_window_bit_length: u32, maximum_symbol_bit_length: u32) -> Vec<u32> {
    let mut window_sizes = Vec::new();
    let (mut current, mut next) = (2u32, 3u32);
    while current <= maximum_symbol_bit_length {
        if current >= minimum_window_bit_length {
            window_sizes.push(current);
        }
        (current, next) = (next, current + next);
    }
    window_sizes
}

pub fn build_symbol_dictionary(stream: &mut SymbolStream, observer: &mut dyn BuilderObserver) {
    let build_start = Instant::now();
    observer.on_build_started(stream);
    for window_bit_length in fibonacci_window_sizes(stream.minimum_window_bit_length, stream.maximum_symbol_bit_length) {
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
        merge_committed_without_lowering_the_total: false,
        leaves_removed: 0,
        leaf_removed_without_lowering_the_total: false,
        single_placement_leaves_removed: 0,
        stopped_by_repeated_state: false,
        merges_evaluated: 0,
        elapsed_milliseconds: 0,
    };
    let mut frequency_states_seen: HashSet<Vec<u64>> = HashSet::new();
    loop {
        summary.passes += 1;
        let changes_this_pass = run_pass(stream, window_bit_length, &mut summary, observer);
        if changes_this_pass == 0 {
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

/// Shared facts about the state at the start of a round.
struct RoundContext {
    window_bit_length: u32,
    pass_number: u32,
    round_number: u32,
    encoded_symbol_bits_before: u64,
    code_lengths_before: Vec<u32>,
    leaves_before: usize,
}

/// Returns the number of changes (merges and leaf removals) committed in this pass.
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
    let mut changes_committed = 0;
    let mut round_number = 0;
    loop {
        round_number += 1;
        let encoded_symbol_bits_before = stream.encoded_symbol_bits();
        let code_lengths_before = stream.code_lengths_by_symbol_id();
        // Candidates are tried on these buffers (a copy of the up-to-date leaf
        // counts, refreshed before each, and reused buffers).
        let mut prediction_buffers = PredictionBuffers::new(stream);
        let context = RoundContext {
            window_bit_length,
            pass_number: summary.passes,
            round_number,
            encoded_symbol_bits_before,
            code_lengths_before,
            leaves_before: stream.leaf_count(),
        };
        if let Some(removal) = find_single_placement_leaf_to_remove(stream, &mut prediction_buffers, &context) {
            report_and_apply_leaf_removal(stream, removal, &context, 0, true, observer);
            summary.leaves_removed += 1;
            summary.single_placement_leaves_removed += 1;
            // Positions changed; the candidates are stale. The next pass rescans.
            return changes_committed + 1;
        }
        let mut removal_scan = find_leaf_to_remove(stream, &mut prediction_buffers, &context);
        if let Some(removal) = removal_scan.removal.take() {
            report_and_apply_leaf_removal(stream, removal, &context, removal_scan.non_saving_leaves, false, observer);
            summary.leaves_removed += 1;
            // Positions changed; the candidates are stale. The next pass rescans.
            return changes_committed + 1;
        }
        let ranked_merges = predict_and_rank_merges(
            stream,
            &mut prediction_buffers,
            &merge_candidates,
            &context,
            &removal_scan,
            &position_consumed_this_pass,
            &mut candidate_finished,
            observer,
        );
        let outcome = commit_best_merge(
            stream,
            ranked_merges,
            &context,
            &removal_scan,
            &mut position_consumed_this_pass,
            &mut candidate_finished,
            summary,
            observer,
        );
        match outcome {
            RoundOutcome::NothingCommitted => break,
            RoundOutcome::MergeCommitted => {
                changes_committed += 1;
                summary.merges_committed += 1;
            }
            RoundOutcome::LeafRemovalForced => {
                let removal = removal_scan.best_removal_not_lowering.take().expect("a forced removal was predicted");
                summary.leaf_removed_without_lowering_the_total = true;
                report_and_apply_leaf_removal(stream, removal, &context, removal_scan.non_saving_leaves, false, observer);
                summary.leaves_removed += 1;
                // Positions changed; the candidates are stale. The next pass rescans.
                stream.remove_vacated_positions();
                return changes_committed + 1;
            }
        }
    }
    stream.remove_vacated_positions();
    changes_committed
}

struct LeafRemovalScan {
    non_saving_leaves: usize,
    non_saving_leaves_unspellable: usize,
    removal: Option<LeafRemovalPrediction>,
    /// When no removal lowers the total: the removal with the lowest total
    /// after (first in scan order on ties), for the forced pick.
    best_removal_not_lowering: Option<LeafRemovalPrediction>,
}

/// Leaf removal rule: the total encoded symbol bits must strictly drop.
fn leaf_removal_lowers_encoded_symbol_bits(removal: &LeafRemovalPrediction, context: &RoundContext) -> bool {
    removal.encoded_symbol_bits_after < context.encoded_symbol_bits_before
}

/// Analysis + decision: the first leaf placed only once (lowest symbol id)
/// that can be respelled with other live leaves.
fn find_single_placement_leaf_to_remove(
    stream: &SymbolStream,
    prediction_buffers: &mut PredictionBuffers,
    context: &RoundContext,
) -> Option<LeafRemovalPrediction> {
    stream
        .symbol_frequencies
        .iter()
        .enumerate()
        .filter(|&(_, &frequency)| frequency == 1)
        .find_map(|(symbol_id, _)| predict_leaf_removal(stream, prediction_buffers, symbol_id, &context.code_lengths_before))
}

/// Analysis + decision: the first non-saving leaf (biggest loss first) that
/// can be respelled and is worth removing.
fn find_leaf_to_remove(
    stream: &SymbolStream,
    prediction_buffers: &mut PredictionBuffers,
    context: &RoundContext,
) -> LeafRemovalScan {
    let non_saving_leaf_ids = find_non_saving_leaves(stream, &context.code_lengths_before);
    let mut scan = LeafRemovalScan {
        non_saving_leaves: non_saving_leaf_ids.len(),
        non_saving_leaves_unspellable: 0,
        removal: None,
        best_removal_not_lowering: None,
    };
    for symbol_id in non_saving_leaf_ids {
        match predict_leaf_removal(stream, prediction_buffers, symbol_id, &context.code_lengths_before) {
            None => scan.non_saving_leaves_unspellable += 1,
            Some(removal) => {
                if leaf_removal_lowers_encoded_symbol_bits(&removal, context) {
                    scan.removal = Some(removal);
                    scan.best_removal_not_lowering = None;
                    break;
                }
                let lower_than_best = match &scan.best_removal_not_lowering {
                    None => true,
                    Some(best) => removal.encoded_symbol_bits_after < best.encoded_symbol_bits_after,
                };
                if lower_than_best {
                    scan.best_removal_not_lowering = Some(removal);
                }
            }
        }
    }
    scan
}

fn report_and_apply_leaf_removal(
    stream: &mut SymbolStream,
    removal: LeafRemovalPrediction,
    context: &RoundContext,
    non_saving_leaves: usize,
    removed_because_placed_once: bool,
    observer: &mut dyn BuilderObserver,
) {
    let event = LeafRemoved {
        window_bit_length: context.window_bit_length,
        pass_number: context.pass_number,
        round_number: context.round_number,
        removed_symbol_id: removal.removed_symbol_id,
        removed_frequency: removal.removed_frequency,
        removed_bit_length: stream.symbol_definitions[removal.removed_symbol_id].bit_length,
        removed_code_length_before: context.code_lengths_before[removal.removed_symbol_id],
        removed_code_bits_before: removal.removed_code_bits_before,
        replacement_symbol_ids: removal.replacement_symbol_ids.clone(),
        replacement_code_bits_after: if observer.wants_code_lengths_after() {
            code_bits_after_changes(stream, &removal.frequency_changes, &removal.replacement_symbol_ids, removal.removed_frequency)
        } else {
            0
        },
        encoded_symbol_bits_before: context.encoded_symbol_bits_before,
        encoded_symbol_bits_after: removal.encoded_symbol_bits_after,
        leaves_before: context.leaves_before,
        non_saving_leaves,
        lowered_the_total: removal.encoded_symbol_bits_after < context.encoded_symbol_bits_before,
        removed_because_placed_once,
    };
    stream.apply_leaf_removal(&removal);
    observer.on_leaf_removed(&event, stream);
}

/// Analysis for one round: every remaining candidate's merge is predicted,
/// and the merges come back ranked best first.
fn predict_and_rank_merges(
    stream: &SymbolStream,
    prediction_buffers: &mut PredictionBuffers,
    merge_candidates: &[MergeCandidate],
    context: &RoundContext,
    removal_scan: &LeafRemovalScan,
    position_consumed_this_pass: &[bool],
    candidate_finished: &mut [bool],
    observer: &mut dyn BuilderObserver,
) -> Vec<MergePrediction> {
    let mut round = RoundAnalyzed {
        window_bit_length: context.window_bit_length,
        pass_number: context.pass_number,
        round_number: context.round_number,
        merges_ranked: 0,
        non_saving_leaves: removal_scan.non_saving_leaves,
        non_saving_leaves_unspellable: removal_scan.non_saving_leaves_unspellable,
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
        let merge =
            predict_merge(stream, prediction_buffers, candidate_index, candidate.definition, occurrence_ranges, &context.code_lengths_before);
        round.merges_that_lower_the_leaf_count +=
            (merge.leaves_after < context.leaves_before) as u64;
        merges.push(merge);
    }
    merges.sort_by(|first, second| {
        compare_by_rank(
            context.encoded_symbol_bits_before,
            &first.ranking_key(),
            &second.ranking_key(),
            &stream.shared_bit_sequence,
        )
    });
    round.merges_ranked = merges.len();
    observer.on_round_analyzed(&round, stream);
    merges
}

/// Merge rule: a merge is preferred when the total encoded symbol bits drop.
fn merge_lowers_encoded_symbol_bits(merge: &MergePrediction, context: &RoundContext) -> bool {
    merge.encoded_symbol_bits_after < context.encoded_symbol_bits_before
}

enum RoundOutcome {
    MergeCommitted,
    /// The forced pick was a leaf removal; the caller applies it.
    LeafRemovalForced,
    NothingCommitted,
}

enum ForcedChoice {
    Merge(usize),
    LeafRemoval,
}

/// Best point: the change of the whole pool with the lowest total after;
/// ties: fewer leaves after, then the removal, then merge rank order.
fn choose_forced_change(ranked_merges: &[MergePrediction], removal_scan: &LeafRemovalScan) -> Option<ForcedChoice> {
    let mut best: Option<(u64, usize, ForcedChoice)> = removal_scan.best_removal_not_lowering.as_ref().map(|removal| {
        (removal.encoded_symbol_bits_after, removal.leaves_after, ForcedChoice::LeafRemoval)
    });
    for (rank_position, merge) in ranked_merges.iter().enumerate() {
        let key = (merge.encoded_symbol_bits_after, merge.leaves_after);
        let better = match &best {
            None => true,
            Some((best_bits, best_leaves, _)) => key.cmp(&(*best_bits, *best_leaves)) == Ordering::Less,
        };
        if better {
            best = Some((key.0, key.1, ForcedChoice::Merge(rank_position)));
        }
    }
    best.map(|(_, _, choice)| choice)
}

/// Decision + application: commit the highest-ranked merge that lowers the
/// total. If none does and this window size has neither committed a merge
/// nor forced a change yet, force the best change of the whole pool (best
/// point): a merge is committed here, a leaf removal is handed back.
fn commit_best_merge(
    stream: &mut SymbolStream,
    ranked_merges: Vec<MergePrediction>,
    context: &RoundContext,
    removal_scan: &LeafRemovalScan,
    position_consumed_this_pass: &mut [bool],
    candidate_finished: &mut [bool],
    summary: &mut WindowSizeFinished,
    observer: &mut dyn BuilderObserver,
) -> RoundOutcome {
    let first_lowering_rank_position =
        ranked_merges.iter().position(|merge| merge_lowers_encoded_symbol_bits(merge, context));
    let forced_change_allowed = summary.merges_committed == 0 && !summary.leaf_removed_without_lowering_the_total;
    let mut leaf_removal_forced = false;
    let chosen_rank_position = match first_lowering_rank_position {
        Some(rank_position) => Some(rank_position),
        None if forced_change_allowed => match choose_forced_change(&ranked_merges, removal_scan) {
            Some(ForcedChoice::Merge(rank_position)) => Some(rank_position),
            Some(ForcedChoice::LeafRemoval) => {
                leaf_removal_forced = true;
                None
            }
            None => None,
        },
        None => None,
    };
    let evaluated_count = match first_lowering_rank_position {
        Some(rank_position) => rank_position + 1,
        None => ranked_merges.len(),
    };
    for (rank_position, merge) in ranked_merges.iter().enumerate().take(evaluated_count) {
        summary.merges_evaluated += 1;
        candidate_finished[merge.candidate_index] = true;
        if Some(rank_position) != chosen_rank_position {
            let rejected = MergeRejected {
                window_bit_length: context.window_bit_length,
                pass_number: context.pass_number,
                round_number: context.round_number,
                rank_position,
                merged_definition: merge.merged_definition,
                frequency: merge.merged_occurrence_ranges.len(),
                reason: RejectionReason::EncodedSymbolBitsDoNotDrop,
                merged_code_bits_after: merged_code_bits_after(stream, merge, observer.wants_code_lengths_after()),
                consumed_code_bits_before: merge.consumed_code_bits_before,
            };
            observer.on_merge_rejected(&rejected, stream);
        }
    }
    if leaf_removal_forced {
        return RoundOutcome::LeafRemovalForced;
    }
    let Some(chosen_rank_position) = chosen_rank_position else { return RoundOutcome::NothingCommitted };
    let merge = &ranked_merges[chosen_rank_position];
    if !merge_lowers_encoded_symbol_bits(merge, context) {
        summary.merge_committed_without_lowering_the_total = true;
    }
    let committed =
        describe_merge_before_applying(stream, merge, context, chosen_rank_position, observer.wants_code_lengths_after());
    stream.apply_merge(merge, position_consumed_this_pass);
    observer.on_change_committed(&committed, stream);
    RoundOutcome::MergeCommitted
}

/// Occurrences * the merged symbol's code length after the merge; for the log
/// only, so 0 unless the observer asks (see BuilderObserver).
fn merged_code_bits_after(stream: &SymbolStream, merge: &MergePrediction, wanted: bool) -> u64 {
    if !wanted {
        return 0;
    }
    let occurrences = merge.merged_occurrence_ranges.len() as u64;
    code_bits_after_changes(stream, &merge.frequency_changes, &[merge.merged_symbol_id as u32], occurrences)
}

/// Builds the full record of a merge from the state before it is applied.
fn describe_merge_before_applying(
    stream: &SymbolStream,
    merge: &MergePrediction,
    context: &RoundContext,
    rank_position: usize,
    wants_code_lengths_after: bool,
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
                    frequency_after_merge: frequency_after_changes(
                        &merge.frequency_changes,
                        consumed_symbol_id,
                        stream.symbol_frequencies[consumed_symbol_id],
                    ),
                }),
            }
        }
    }
    let (removed_leaf_ids, added_leaf_ids) =
        list_leaves_removed_and_added(&stream.symbol_frequencies, &merge.frequency_changes);
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
        lowered_the_total: merge.encoded_symbol_bits_after < context.encoded_symbol_bits_before,
        frequency: merge.merged_occurrence_ranges.len(),
        consumed_parts,
        merged_code_bits_after: merged_code_bits_after(stream, merge, wants_code_lengths_after),
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
