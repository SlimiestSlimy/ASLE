//! Which Huffman code length each symbol has, stored exactly:
//! * Elias gamma(longest code length)
//! * Elias gamma(count + 1) for every code length 1..=longest
//! * one multiset-permutation index: the sequence of code lengths in the
//!   symbols' depth-first order, ranked among all sequences with those counts,
//!   written in exactly enough bits for (number of sequences - 1).
//! Symbols with equal code lengths are interchangeable in a canonical code,
//! so this is all the decoder needs to rebuild every code.

use crate::big_unsigned_integer::BigUnsignedInteger;
use crate::bit_io::{BitReader, BitWriter};
use crate::multiset_permutation::{count_multiset_permutations, rank_multiset_permutation, unrank_multiset_permutation};

fn elias_gamma_bit_count(value: u64) -> u64 {
    2 * (64 - value.leading_zeros() as u64) - 1
}

/// How many symbols have each code length; index 0 is code length 1.
fn count_symbols_by_code_length(code_lengths_in_order: &[u32]) -> Vec<u64> {
    let longest_code_length = code_lengths_in_order.iter().copied().max().unwrap_or(0);
    let mut symbol_count_by_code_length = vec![0u64; longest_code_length as usize];
    for &code_length in code_lengths_in_order {
        symbol_count_by_code_length[code_length as usize - 1] += 1;
    }
    symbol_count_by_code_length
}

fn permutation_index_bit_count(symbol_count_by_code_length: &[u64]) -> u64 {
    let mut largest_index = count_multiset_permutations(symbol_count_by_code_length);
    largest_index.subtract_assign(&BigUnsignedInteger::from_u64(1));
    largest_index.bit_length()
}

pub fn measure_code_length_section_bit_count(code_lengths_in_order: &[u32]) -> u64 {
    let symbol_count_by_code_length = count_symbols_by_code_length(code_lengths_in_order);
    elias_gamma_bit_count(symbol_count_by_code_length.len() as u64)
        + symbol_count_by_code_length.iter().map(|&count| elias_gamma_bit_count(count + 1)).sum::<u64>()
        + permutation_index_bit_count(&symbol_count_by_code_length)
}

pub fn write_code_length_section(bit_writer: &mut BitWriter, code_lengths_in_order: &[u32]) {
    let symbol_count_by_code_length = count_symbols_by_code_length(code_lengths_in_order);
    bit_writer.write_elias_gamma(symbol_count_by_code_length.len() as u64);
    for &count in &symbol_count_by_code_length {
        bit_writer.write_elias_gamma(count + 1);
    }
    let length_groups: Vec<usize> = code_lengths_in_order.iter().map(|&code_length| code_length as usize - 1).collect();
    let rank = rank_multiset_permutation(&length_groups, symbol_count_by_code_length.len());
    rank.write_bits(bit_writer, permutation_index_bit_count(&symbol_count_by_code_length));
}

pub fn read_code_length_section(bit_reader: &mut BitReader) -> Vec<u32> {
    let longest_code_length = bit_reader.read_elias_gamma();
    let symbol_count_by_code_length: Vec<u64> =
        (0..longest_code_length).map(|_| bit_reader.read_elias_gamma() - 1).collect();
    let rank = BigUnsignedInteger::read_bits(bit_reader, permutation_index_bit_count(&symbol_count_by_code_length));
    unrank_multiset_permutation(rank, &symbol_count_by_code_length)
        .into_iter()
        .map(|group| group as u32 + 1)
        .collect()
}
