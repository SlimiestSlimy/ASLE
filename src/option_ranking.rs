//! Decision: how merge options are scored and ordered. Integer math only.
//!
//! score = frequency * (1 + (bits_before - bits_final) / bits_before)
//!         * (leaves removed / leaves added, only when the leaf count drops)
//!
//! where bits = encoded symbol bits. bits_before is the same for every option
//! in a round, so scores compare as the exact fraction
//! frequency * (2 bits_before - bits_final) * removed / added.
//! A fraction x/0 (leaves removed, none added) ranks above every finite one.
//! Ties: lower bits_final, then higher frequency, then smaller bit value.

use crate::predicted_changes::{LeafChange, MergeOption};
use std::cmp::Ordering;

pub struct RankingKey {
    pub frequency: u64,
    pub merged_bits: u64,
    pub leaf_change_if_shrinking: Option<LeafChange>,
    pub final_encoded_symbol_bits: u64,
}

impl MergeOption {
    pub fn ranking_key(&self) -> RankingKey {
        RankingKey {
            frequency: self.merge.merged_occurrence_ranges.len() as u64,
            merged_bits: self.merge.merged_definition.bits,
            leaf_change_if_shrinking: self.leaf_change_if_shrinking,
            final_encoded_symbol_bits: self.final_encoded_symbol_bits,
        }
    }
}

/// (numerator, denominator) of the score, without the shared 1/bits_before.
pub fn calculate_score_fraction(encoded_symbol_bits_before: u64, key: &RankingKey) -> (i128, i128) {
    let base_score = key.frequency as i128 * (2 * encoded_symbol_bits_before as i128 - key.final_encoded_symbol_bits as i128);
    match key.leaf_change_if_shrinking {
        Some(leaf_change) => (base_score * leaf_change.leaves_removed as i128, leaf_change.leaves_added as i128),
        None => (base_score, 1),
    }
}

/// Ordering::Less means `first` ranks higher than `second`.
pub fn compare_by_rank(encoded_symbol_bits_before: u64, first: &RankingKey, second: &RankingKey) -> Ordering {
    let (first_numerator, first_denominator) = calculate_score_fraction(encoded_symbol_bits_before, first);
    let (second_numerator, second_denominator) = calculate_score_fraction(encoded_symbol_bits_before, second);
    let score_ordering = match (first_denominator == 0, second_denominator == 0) {
        (true, true) => first_numerator.cmp(&second_numerator),
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => (first_numerator * second_denominator).cmp(&(second_numerator * first_denominator)),
    };
    score_ordering
        .reverse()
        .then(first.final_encoded_symbol_bits.cmp(&second.final_encoded_symbol_bits))
        .then(second.frequency.cmp(&first.frequency))
        .then(first.merged_bits.cmp(&second.merged_bits))
}
