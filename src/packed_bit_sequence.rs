//! A growable sequence of bits packed into 64-bit words, most significant bit
//! first. Symbols are slices of one shared sequence (offset, bit length), so
//! symbol length has no limit and symbols never copy their bits.

use std::cmp::Ordering;

pub struct PackedBitSequence {
    words: Vec<u64>,
    bit_count: u64,
}

const WORD_BITS: u64 = 64;

impl PackedBitSequence {
    pub fn new() -> Self {
        PackedBitSequence { words: Vec::new(), bit_count: 0 }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut sequence = PackedBitSequence::new();
        for &byte in bytes {
            for bit_position in (0..8).rev() {
                sequence.push_bit(((byte >> bit_position) & 1) as u64);
            }
        }
        sequence
    }

    pub fn bit_count(&self) -> u64 {
        self.bit_count
    }

    pub fn push_bit(&mut self, bit: u64) {
        let bit_in_word = self.bit_count % WORD_BITS;
        if bit_in_word == 0 {
            self.words.push(0);
        }
        if bit & 1 == 1 {
            *self.words.last_mut().unwrap() |= 1u64 << (WORD_BITS - 1 - bit_in_word);
        }
        self.bit_count += 1;
    }

    pub fn read_bit(&self, bit_offset: u64) -> u64 {
        let word = self.words[(bit_offset / WORD_BITS) as usize];
        (word >> (WORD_BITS - 1 - bit_offset % WORD_BITS)) & 1
    }

    /// Up to 64 bits starting at `bit_offset`, right-aligned in the result.
    pub fn read_bits(&self, bit_offset: u64, bit_count: u32) -> u64 {
        assert!(bit_count <= 64);
        if bit_count == 0 {
            return 0;
        }
        let word_index = (bit_offset / WORD_BITS) as usize;
        let bit_in_word = (bit_offset % WORD_BITS) as u32;
        let mut left_aligned = self.words[word_index] << bit_in_word;
        if bit_in_word > 0 && bit_in_word + bit_count > 64 {
            left_aligned |= self.words[word_index + 1] >> (64 - bit_in_word);
        }
        left_aligned >> (64 - bit_count)
    }

    /// Calls `visit_chunk(chunk_value, chunk_bit_count)` for consecutive chunks
    /// of at most 64 bits covering the slice, in order.
    pub fn for_each_chunk(&self, bit_offset: u64, bit_length: u64, mut visit_chunk: impl FnMut(u64, u32)) {
        let mut done = 0u64;
        while done < bit_length {
            let chunk_bit_count = (bit_length - done).min(WORD_BITS) as u32;
            visit_chunk(self.read_bits(bit_offset + done, chunk_bit_count), chunk_bit_count);
            done += chunk_bit_count as u64;
        }
    }

    /// A deterministic hash of the slice's length and content.
    pub fn hash_slice(&self, bit_offset: u64, bit_length: u64) -> u64 {
        let mut hash = bit_length.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        self.for_each_chunk(bit_offset, bit_length, |chunk_value, _| {
            hash = (hash.rotate_left(5) ^ chunk_value).wrapping_mul(0x517c_c1b7_2722_0a95);
        });
        hash
    }

    /// Exact content comparison of two slices of the same length.
    pub fn slices_are_equal(&self, first_offset: u64, second_offset: u64, bit_length: u64) -> bool {
        let mut done = 0u64;
        while done < bit_length {
            let chunk_bit_count = (bit_length - done).min(WORD_BITS) as u32;
            if self.read_bits(first_offset + done, chunk_bit_count)
                != self.read_bits(second_offset + done, chunk_bit_count)
            {
                return false;
            }
            done += chunk_bit_count as u64;
        }
        true
    }

    /// Depth-first (prefix tree) order: bit by bit, 0 before 1, and a slice
    /// that is a prefix of another comes first.
    pub fn compare_slices_depth_first(
        &self,
        first_offset: u64,
        first_length: u64,
        second_offset: u64,
        second_length: u64,
    ) -> Ordering {
        let common_length = first_length.min(second_length);
        let mut done = 0u64;
        while done < common_length {
            let chunk_bit_count = (common_length - done).min(WORD_BITS) as u32;
            let first_chunk = self.read_bits(first_offset + done, chunk_bit_count);
            let second_chunk = self.read_bits(second_offset + done, chunk_bit_count);
            if first_chunk != second_chunk {
                return first_chunk.cmp(&second_chunk);
            }
            done += chunk_bit_count as u64;
        }
        first_length.cmp(&second_length)
    }

    pub fn slice_as_bit_string(&self, bit_offset: u64, bit_length: u64) -> String {
        (0..bit_length).map(|index| if self.read_bit(bit_offset + index) == 1 { '1' } else { '0' }).collect()
    }
}
