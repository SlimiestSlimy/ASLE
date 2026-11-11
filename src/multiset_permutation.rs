//! Exact ranking of multiset permutations: a sequence of group numbers in
//! which group g appears `group_counts[g]` times is mapped to one number in
//! [0, number of distinct such sequences), and back.
//!
//! The number of sequences is the multinomial L! / (c0! c1! ...). Ranking goes
//! position by position: the sequences that put a smaller group at this
//! position come first. All divisions below are exact.

use crate::big_unsigned_integer::BigUnsignedInteger;
use std::cmp::Ordering;

/// L! / (c0! c1! ...), built one placed element at a time with small factors.
pub fn count_multiset_permutations(group_counts: &[u64]) -> BigUnsignedInteger {
    let mut permutation_count = BigUnsignedInteger::from_u64(1);
    let mut placed_so_far = 0u64;
    for &group_count in group_counts {
        // permutation_count stays the multinomial of the groups placed so far,
        // times C(placed + step, step) after each step: every division is exact.
        for step in 1..=group_count {
            permutation_count.multiply_by_small(placed_so_far + step);
            permutation_count.divide_by_small(step);
        }
        placed_so_far += group_count;
    }
    permutation_count
}

/// Number of sequences of the remaining multiset that start with `group`:
/// all remaining sequences * count of group / remaining length.
fn sequences_starting_with(
    remaining_sequences: &BigUnsignedInteger,
    group_count: u64,
    remaining_length: u64,
) -> BigUnsignedInteger {
    let mut sequences = remaining_sequences.clone();
    sequences.multiply_by_small(group_count);
    sequences.divide_by_small(remaining_length);
    sequences
}

pub fn rank_multiset_permutation(sequence: &[usize], group_count_total: usize) -> BigUnsignedInteger {
    let mut remaining_counts = vec![0u64; group_count_total];
    for &group in sequence {
        remaining_counts[group] += 1;
    }
    let mut remaining_sequences = count_multiset_permutations(&remaining_counts);
    let mut rank = BigUnsignedInteger::zero();
    for (position, &group) in sequence.iter().enumerate() {
        let remaining_length = (sequence.len() - position) as u64;
        for smaller_group in 0..group {
            if remaining_counts[smaller_group] > 0 {
                rank.add_assign(&sequences_starting_with(
                    &remaining_sequences,
                    remaining_counts[smaller_group],
                    remaining_length,
                ));
            }
        }
        remaining_sequences = sequences_starting_with(&remaining_sequences, remaining_counts[group], remaining_length);
        remaining_counts[group] -= 1;
    }
    rank
}

pub fn unrank_multiset_permutation(mut rank: BigUnsignedInteger, group_counts: &[u64]) -> Vec<usize> {
    let mut remaining_counts = group_counts.to_vec();
    let sequence_length: u64 = group_counts.iter().sum();
    let mut remaining_sequences = count_multiset_permutations(&remaining_counts);
    let mut sequence = Vec::with_capacity(sequence_length as usize);
    for position in 0..sequence_length {
        let remaining_length = sequence_length - position;
        for group in 0..remaining_counts.len() {
            if remaining_counts[group] == 0 {
                continue;
            }
            let block = sequences_starting_with(&remaining_sequences, remaining_counts[group], remaining_length);
            if rank.compare(&block) == Ordering::Less {
                sequence.push(group);
                remaining_sequences = block;
                remaining_counts[group] -= 1;
                break;
            }
            rank.subtract_assign(&block);
        }
    }
    sequence
}
