//! A binary range coder (arithmetic coding with integers), in the style of
//! LZMA's: a 64-bit low end, a 32-bit range, carries resolved through a
//! cached byte. Probabilities of a 0 are u32 fractions of 2^32; the split is
//! (range * probability) >> 32 in 64-bit integers. Integer math only, so the
//! encoder and the decoder agree bit for bit.
//!
//! The coder's very first output byte is always 0; it is left out of the
//! stream and the decoder starts from it implicitly (MINE: saves 8 bits).

use crate::bit_io::{BitReader, BitWriter};

/// A probability p stands for p / 2^32.
pub type Probability = u32;
/// One half: 2^31 / 2^32.
pub const PROBABILITY_HALF: Probability = 1 << 31;
const TOP: u32 = 1 << 24;

/// Where the interval splits: the 0 side gets (range * probability) >> 32.
fn split_point(range: u32, probability_of_zero: Probability) -> u32 {
    let bound = ((range as u64 * probability_of_zero as u64) >> 32) as u32;
    assert!(bound > 0 && bound < range, "probability too extreme for the current range");
    bound
}

pub struct BinaryRangeEncoder {
    low: u64,
    range: u32,
    cache: u8,
    cache_size: u64,
    output_bytes: Vec<u8>,
    first_byte_skipped: bool,
}

impl BinaryRangeEncoder {
    pub fn new() -> Self {
        BinaryRangeEncoder { low: 0, range: u32::MAX, cache: 0, cache_size: 1, output_bytes: Vec::new(), first_byte_skipped: false }
    }

    fn output_byte(&mut self, byte: u8) {
        if !self.first_byte_skipped {
            assert_eq!(byte, 0, "a range coder's first byte is always 0");
            self.first_byte_skipped = true;
        } else {
            self.output_bytes.push(byte);
        }
    }

    fn shift_low(&mut self) {
        if (self.low as u32) < 0xFF00_0000 || (self.low >> 32) != 0 {
            let carry = (self.low >> 32) as u8;
            let mut pending = self.cache;
            loop {
                self.output_byte(pending.wrapping_add(carry));
                pending = 0xFF;
                self.cache_size -= 1;
                if self.cache_size == 0 {
                    break;
                }
            }
            self.cache = ((self.low >> 24) & 0xFF) as u8;
        }
        self.cache_size += 1;
        self.low = (self.low & 0x00FF_FFFF) << 8;
    }

    /// Codes `bit` given the probability of a 0.
    pub fn encode_bit(&mut self, probability_of_zero: Probability, bit: u64) {
        let bound = split_point(self.range, probability_of_zero);
        if bit == 0 {
            self.range = bound;
        } else {
            self.low += bound as u64;
            self.range -= bound;
        }
        while self.range < TOP {
            self.range <<= 8;
            self.shift_low();
        }
    }

    /// Flushes and writes every coded byte to `bit_writer`; returns the bits written.
    pub fn finish_into(mut self, bit_writer: &mut BitWriter) -> u64 {
        for _ in 0..5 {
            self.shift_low();
        }
        for &byte in &self.output_bytes {
            bit_writer.write_bits(byte as u64, 8);
        }
        8 * self.output_bytes.len() as u64
    }
}

pub struct BinaryRangeDecoder {
    range: u32,
    code: u32,
}

impl BinaryRangeDecoder {
    /// Starts reading coded bytes from `bit_reader` (bits past the stream's end read as 0).
    pub fn start(bit_reader: &mut BitReader) -> Self {
        let mut code = 0u32; // the implicit first byte, 0
        for _ in 0..4 {
            code = (code << 8) | bit_reader.read_bits_or_zeros_past_end(8) as u32;
        }
        BinaryRangeDecoder { range: u32::MAX, code }
    }

    pub fn decode_bit(&mut self, probability_of_zero: Probability, bit_reader: &mut BitReader) -> u64 {
        let bound = split_point(self.range, probability_of_zero);
        let bit = if self.code < bound {
            self.range = bound;
            0
        } else {
            self.code -= bound;
            self.range -= bound;
            1
        };
        while self.range < TOP {
            self.range <<= 8;
            self.code = (self.code << 8) | bit_reader.read_bits_or_zeros_past_end(8) as u32;
        }
        bit
    }
}
