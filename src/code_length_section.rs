//! Which Huffman code length each symbol has, stored exactly:
//! * for code length 1, 2, 3, ...: how many symbols have it, written in
//!   exactly enough bits for the values the code tree still allows, until no
//!   symbol is left (so the longest code length is never written);
//! * one multiset-permutation index: the code lengths in the symbols'
//!   depth-first order, ranked among all orderings with those counts, written
//!   in exactly enough bits for (number of orderings - 1).
//!
//! The allowed values (the user's design, 2026-11-10, with the range from the
//! code tree's shape): the canonical code tree is full (every node has 0 or 2
//! children). At code length d there are `slots` free places (2 at length 1,
//! then twice the places not taken by leaves); every place not taken by a leaf
//! needs at least 2 leaves below it. So with `remaining` symbols not yet
//! placed, the count lies in [max(0, 2 slots - remaining), min(remaining, slots)].
//!
//! The symbol count comes from the prefix tree. A lone symbol has code length
//! 1 (the root's left child) and nothing is written for it.

use crate::big_unsigned_integer::BigUnsignedInteger;
use crate::bit_io::{BitReader, BitWriter};
use crate::multiset_permutation::{count_multiset_permutations, rank_multiset_permutation, unrank_multiset_permutation};

/// How many symbols have each code length; index 0 is code length 1.
fn count_symbols_by_code_length(code_lengths_in_order: &[u32]) -> Vec<u64> {
    let longest_code_length = code_lengths_in_order.iter().copied().max().unwrap_or(0);
    let mut symbol_count_by_code_length = vec![0u64; longest_code_length as usize];
    for &code_length in code_lengths_in_order {
        symbol_count_by_code_length[code_length as usize - 1] += 1;
    }
    symbol_count_by_code_length
}

/// The values the count at the next code length may take: (lowest, highest).
fn allowed_count_range(free_places: u64, symbols_remaining: u64) -> (u64, u64) {
    ((2 * free_places).saturating_sub(symbols_remaining), symbols_remaining.min(free_places))
}

fn bits_for_range(lowest: u64, highest: u64) -> u32 {
    64 - (highest - lowest).leading_zeros()
}

/// Calls `visit(count, lowest, highest)` for every written count, in order.
fn walk_counts(symbol_count_by_code_length: &[u64], symbol_count: u64, mut visit: impl FnMut(u64, u64, u64)) {
    if symbol_count <= 1 {
        return;
    }
    let mut free_places = 2u64;
    let mut symbols_remaining = symbol_count;
    for &count in symbol_count_by_code_length {
        let (lowest, highest) = allowed_count_range(free_places, symbols_remaining);
        assert!(lowest <= count && count <= highest, "code lengths do not form a full code tree");
        visit(count, lowest, highest);
        symbols_remaining -= count;
        free_places = 2 * (free_places - count);
        if symbols_remaining == 0 {
            break;
        }
    }
}

fn permutation_index_bit_count(symbol_count_by_code_length: &[u64]) -> u64 {
    let mut largest_index = count_multiset_permutations(symbol_count_by_code_length);
    largest_index.subtract_assign(&BigUnsignedInteger::from_u64(1));
    largest_index.bit_length()
}

pub fn measure_code_length_section_bit_count(code_lengths_in_order: &[u32]) -> u64 {
    let symbol_count_by_code_length = count_symbols_by_code_length(code_lengths_in_order);
    let mut count_bits = 0u64;
    walk_counts(&symbol_count_by_code_length, code_lengths_in_order.len() as u64, |_, lowest, highest| {
        count_bits += bits_for_range(lowest, highest) as u64;
    });
    count_bits + permutation_index_bit_count(&symbol_count_by_code_length)
}

pub fn write_code_length_section(bit_writer: &mut BitWriter, code_lengths_in_order: &[u32]) {
    let symbol_count_by_code_length = count_symbols_by_code_length(code_lengths_in_order);
    walk_counts(&symbol_count_by_code_length, code_lengths_in_order.len() as u64, |count, lowest, highest| {
        bit_writer.write_bits(count - lowest, bits_for_range(lowest, highest));
    });
    let length_groups: Vec<usize> = code_lengths_in_order.iter().map(|&code_length| code_length as usize - 1).collect();
    let rank = rank_multiset_permutation(&length_groups, symbol_count_by_code_length.len());
    rank.write_bits(bit_writer, permutation_index_bit_count(&symbol_count_by_code_length));
}

/// Reads the code lengths of `symbol_count` symbols (known from the prefix tree).
pub fn read_code_length_section(bit_reader: &mut BitReader, symbol_count: u64) -> Vec<u32> {
    if symbol_count == 1 {
        return vec![1];
    }
    let mut symbol_count_by_code_length = Vec::new();
    let mut free_places = 2u64;
    let mut symbols_remaining = symbol_count;
    while symbols_remaining > 0 {
        let (lowest, highest) = allowed_count_range(free_places, symbols_remaining);
        let count = lowest + bit_reader.read_bits(bits_for_range(lowest, highest));
        symbol_count_by_code_length.push(count);
        symbols_remaining -= count;
        free_places = 2 * (free_places - count);
    }
    let rank = BigUnsignedInteger::read_bits(bit_reader, permutation_index_bit_count(&symbol_count_by_code_length));
    unrank_multiset_permutation(rank, &symbol_count_by_code_length)
        .into_iter()
        .map(|group| group as u32 + 1)
        .collect()
}
