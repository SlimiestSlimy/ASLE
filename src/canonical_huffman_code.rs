//! Canonical Huffman codes from code lengths alone, and their decoding.
//!
//! Codes are handed out in order of (code length, symbol position): shorter
//! codes first, and within one length in the order the symbols are listed
//! (the prefix tree's depth-first order). Given the same lengths in the same
//! order, encoder and decoder build the same codes.

use crate::bit_io::BitReader;

/// (code bits, code length) for every position, from the code lengths.
pub fn assign_canonical_codes(code_lengths: &[u32]) -> Vec<(u64, u32)> {
    let mut positions_in_code_order: Vec<usize> = (0..code_lengths.len()).collect();
    positions_in_code_order.sort_by_key(|&position| (code_lengths[position], position));
    let mut codes = vec![(0u64, 0u32); code_lengths.len()];
    let mut next_code = 0u64;
    let mut previous_length = 0u32;
    for (index, &position) in positions_in_code_order.iter().enumerate() {
        let length = code_lengths[position];
        assert!(length < 64, "code longer than 63 bits");
        if index > 0 {
            next_code += 1;
        }
        next_code <<= length - previous_length;
        previous_length = length;
        codes[position] = (next_code, length);
    }
    codes
}

pub struct CanonicalHuffmanDecoder {
    /// For each code length: the first code of that length and how many there are.
    first_code_by_length: Vec<u64>,
    code_count_by_length: Vec<u64>,
    /// Index into positions_in_code_order where each length's codes start.
    first_index_by_length: Vec<usize>,
    positions_in_code_order: Vec<usize>,
    longest_code_length: u32,
}

impl CanonicalHuffmanDecoder {
    pub fn from_code_lengths(code_lengths: &[u32]) -> Self {
        let longest_code_length = code_lengths.iter().copied().max().unwrap_or(0);
        let mut positions_in_code_order: Vec<usize> = (0..code_lengths.len()).collect();
        positions_in_code_order.sort_by_key(|&position| (code_lengths[position], position));
        let mut code_count_by_length = vec![0u64; longest_code_length as usize + 1];
        for &length in code_lengths {
            code_count_by_length[length as usize] += 1;
        }
        let mut first_code_by_length = vec![0u64; longest_code_length as usize + 1];
        let mut first_index_by_length = vec![0usize; longest_code_length as usize + 1];
        let mut next_code = 0u64;
        let mut next_index = 0usize;
        for length in 1..=longest_code_length as usize {
            next_code = (next_code + code_count_by_length[length - 1]) << 1;
            first_code_by_length[length] = next_code;
            first_index_by_length[length] = next_index;
            next_index += code_count_by_length[length] as usize;
        }
        CanonicalHuffmanDecoder {
            first_code_by_length,
            code_count_by_length,
            first_index_by_length,
            positions_in_code_order,
            longest_code_length,
        }
    }

    /// Reads one code; returns the position (symbol index) it stands for.
    pub fn decode_next_position(&self, bit_reader: &mut BitReader) -> usize {
        let mut code = 0u64;
        for length in 1..=self.longest_code_length as usize {
            code = (code << 1) | bit_reader.read_bit();
            let count = self.code_count_by_length[length];
            let first = self.first_code_by_length[length];
            if count > 0 && code >= first && code - first < count {
                return self.positions_in_code_order[self.first_index_by_length[length] + (code - first) as usize];
            }
        }
        panic!("invalid canonical Huffman code");
    }
}
