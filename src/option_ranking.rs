//! Decision: how merges are scored and ordered. Integer math only.
//!
//! score = frequency * (1 + (bits_before - bits_after) / bits_before)
//! where bits = encoded symbol bits. bits_before is the same for every merge
//! in a round, so scores compare exactly as frequency * (2 bits_before - bits_after).
//! Ties: higher frequency, then smaller bit value.

use crate::predicted_changes::MergePrediction;
use std::cmp::Ordering;

pub struct RankingKey {
    pub frequency: u64,
    pub merged_bits: u64,
    pub encoded_symbol_bits_after: u64,
}

impl MergePrediction {
    pub fn ranking_key(&self) -> RankingKey {
        RankingKey {
            frequency: self.merged_occurrence_ranges.len() as u64,
            merged_bits: self.merged_definition.bits,
            encoded_symbol_bits_after: self.encoded_symbol_bits_after,
        }
    }
}

/// The score without the shared 1 / bits_before.
pub fn calculate_score_numerator(encoded_symbol_bits_before: u64, key: &RankingKey) -> i128 {
    key.frequency as i128 * (2 * encoded_symbol_bits_before as i128 - key.encoded_symbol_bits_after as i128)
}

/// Ordering::Less means `first` ranks higher than `second`.
pub fn compare_by_rank(encoded_symbol_bits_before: u64, first: &RankingKey, second: &RankingKey) -> Ordering {
    let first_score = calculate_score_numerator(encoded_symbol_bits_before, first);
    let second_score = calculate_score_numerator(encoded_symbol_bits_before, second);
    second_score
        .cmp(&first_score)
        .then(second.frequency.cmp(&first.frequency))
        .then(first.merged_bits.cmp(&second.merged_bits))
}
