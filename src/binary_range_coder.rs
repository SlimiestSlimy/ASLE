//! A binary range coder (arithmetic coding with integers) that outputs single
//! bits, not bytes (the user's design, 2026-11-11): a bit leaves the coder
//! only when the range falls below 2^31, i.e. when keeping it would cost
//! precision. So the body is precision shaped, never byte shaped.
//!
//! The low end is a 32-bit window (in a u64, so an addition can carry out of
//! it); the range stays within [2^31, 2^32) between decisions. Probabilities
//! of a 0 are u32 fractions of 2^32; the split is (range * probability) >> 32
//! in 64-bit integers. Integer math only, so the encoder and the decoder agree
//! bit for bit.
//!
//! Bits that have left the window can still be changed by a carry; they are
//! kept in memory (SettledBits) until the end, where the carry is added into
//! them directly (MINE: simpler than counting pending bits).
//!
//! The end (the user's design, 2026-11-11): after the last decision the
//! encoder writes the fewest bits that put the number inside the final range,
//! whatever bits come after them; the decoder knows the symbol count, so it
//! stops after the last one by itself, works out the same final bits from its
//! own copy of the low end, and leaves its reader right after them. No length
//! and no end marker: anything can follow the body.

use crate::bit_io::{BitReader, BitWriter};

/// A probability p stands for p / 2^32.
pub type Probability = u32;
/// One half: 2^31 / 2^32.
pub const PROBABILITY_HALF: Probability = 1 << 31;
/// The range never stays below this between decisions: one more bit leaves
/// the coder whenever it would.
const RANGE_FLOOR: u64 = 1 << 31;
/// The smallest probability either side can get without the split emptying a
/// side: 2^32 / RANGE_FLOOR (a side then still gets at least 1 of the range).
pub const LEAST_PROBABILITY_OF_A_SIDE: u64 = (1 << 32) / RANGE_FLOOR;
const WINDOW: u64 = 1 << 32;

/// Where the interval splits: the 0 side gets (range * probability) >> 32.
fn split_point(range: u64, probability_of_zero: Probability) -> u64 {
    let bound = (range * probability_of_zero as u64) >> 32;
    assert!(bound > 0 && bound < range, "probability too extreme for the current range");
    bound
}

/// The end of the body: the fewest window bits k, and their value v (a
/// multiple of 2^(32 - k), possibly 2^32 or more: a carry), such that every
/// number starting with them, [v, v + 2^(32 - k)), lies in the final range
/// [low, low + range). Whatever follows the body can't move the number out.
/// The range is at least 2^31 here, so k is at most 2.
fn final_window_bits(low: u64, range: u64) -> (u64, u32) {
    (0..=32u32)
        .find_map(|k| {
            let step = 1u64 << (32 - k);
            let value = low.div_ceil(step) * step;
            (value + step <= low + range).then_some((value, k))
        })
        .expect("a range of at least 1 holds a block of size 1")
}

/// The bits that have left the coder's window, in order, packed 64 to a word
/// (first bit at the top of the first word).
#[derive(Default)]
struct SettledBits {
    words: Vec<u64>,
    bit_count: u64,
}

impl SettledBits {
    fn push(&mut self, bit: u64) {
        let bit_in_word = self.bit_count % 64;
        if bit_in_word == 0 {
            self.words.push(0);
        }
        *self.words.last_mut().unwrap() |= bit << (63 - bit_in_word);
        self.bit_count += 1;
    }

    /// Adds 1 at the last settled bit: trailing 1s become 0s, the last 0 a 1.
    fn add_carry(&mut self) {
        let mut position = self.bit_count;
        loop {
            assert!(position > 0, "a carry never runs past the first settled bit");
            position -= 1;
            let (word, mask) = ((position / 64) as usize, 1u64 << (63 - position % 64));
            self.words[word] ^= mask;
            if self.words[word] & mask != 0 {
                break;
            }
        }
    }

    fn write_into(&self, bit_writer: &mut BitWriter) {
        for (index, &word) in self.words.iter().enumerate() {
            let bits_in_word = (self.bit_count - 64 * index as u64).min(64) as u32;
            bit_writer.write_bits(word >> (64 - bits_in_word), bits_in_word);
        }
    }
}

pub struct BinaryRangeEncoder {
    /// The low end, within the window [0, 2^32); a carry out of it goes
    /// straight into the settled bits.
    low: u64,
    range: u64,
    settled_bits: SettledBits,
}

impl BinaryRangeEncoder {
    pub fn new() -> Self {
        BinaryRangeEncoder { low: 0, range: WINDOW - 1, settled_bits: SettledBits::default() }
    }

    fn carry_out_of_the_window(&mut self) {
        if self.low >= WINDOW {
            self.settled_bits.add_carry();
            self.low -= WINDOW;
        }
    }

    /// Codes `bit` given the probability of a 0.
    pub fn encode_bit(&mut self, probability_of_zero: Probability, bit: u64) {
        let bound = split_point(self.range, probability_of_zero);
        if bit == 0 {
            self.range = bound;
        } else {
            self.low += bound;
            self.range -= bound;
            self.carry_out_of_the_window();
        }
        while self.range < RANGE_FLOOR {
            self.settled_bits.push(self.low >> 31);
            self.low = (self.low << 1) & (WINDOW - 1);
            self.range <<= 1;
        }
    }

    /// Ends the body (final_window_bits), writes every bit to `bit_writer`;
    /// returns their count.
    pub fn finish_into(mut self, bit_writer: &mut BitWriter) -> u64 {
        let (value, value_bit_count) = final_window_bits(self.low, self.range);
        self.low = value;
        self.carry_out_of_the_window();
        for position in 0..value_bit_count {
            self.settled_bits.push((self.low >> (31 - position)) & 1);
        }
        self.settled_bits.write_into(bit_writer);
        self.settled_bits.bit_count
    }
}

pub struct BinaryRangeDecoder {
    range: u64,
    /// The read value minus the low end, within [0, range).
    code: u64,
    /// The encoder's low end, within the window (carries don't matter here:
    /// they only change bits already read), to find the body's end.
    low: u64,
}

/// The decoder reads this many bits ahead of the encoder's position.
const DECODER_LOOKAHEAD_BITS: u32 = 32;

impl BinaryRangeDecoder {
    /// Starts reading coded bits from `bit_reader` (bits past the stream's end read as 0).
    pub fn start(bit_reader: &mut BitReader) -> Self {
        let code = bit_reader.read_bits_or_zeros_past_end(DECODER_LOOKAHEAD_BITS);
        BinaryRangeDecoder { range: WINDOW - 1, code, low: 0 }
    }

    /// After the last decision: works out the encoder's final bits and puts
    /// `bit_reader` right after them, where whatever follows the body starts.
    pub fn finish(self, bit_reader: &mut BitReader) {
        let (_, value_bit_count) = final_window_bits(self.low, self.range);
        bit_reader.move_back((DECODER_LOOKAHEAD_BITS - value_bit_count) as u64);
    }

    pub fn decode_bit(&mut self, probability_of_zero: Probability, bit_reader: &mut BitReader) -> u64 {
        let bound = split_point(self.range, probability_of_zero);
        let bit = if self.code < bound {
            self.range = bound;
            0
        } else {
            self.code -= bound;
            self.low = (self.low + bound) & (WINDOW - 1);
            self.range -= bound;
            1
        };
        while self.range < RANGE_FLOOR {
            self.low = (self.low << 1) & (WINDOW - 1);
            self.range <<= 1;
            self.code = (self.code << 1) | bit_reader.read_bits_or_zeros_past_end(1);
        }
        bit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Random bits under random (often extreme) probabilities come back
    /// exactly, with random bits right after the body: the decoder stops
    /// exactly where the body ends, and what follows reads back.
    #[test]
    fn bits_round_trip() {
        let mut state = 7u64;
        let mut next = || {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        };
        for trial in 0..300 {
            let decision_count = (next() % 3000) as usize + trial % 3;
            let decisions: Vec<(Probability, u64)> = (0..decision_count)
                .map(|_| {
                    let probability = match next() % 4 {
                        0 => LEAST_PROBABILITY_OF_A_SIDE,
                        1 => WINDOW - LEAST_PROBABILITY_OF_A_SIDE,
                        _ => (next() % (WINDOW - 2 * LEAST_PROBABILITY_OF_A_SIDE)) + LEAST_PROBABILITY_OF_A_SIDE,
                    } as Probability;
                    // Mostly the likely side, so long confident runs happen.
                    let likely = if probability >= PROBABILITY_HALF { 0 } else { 1 };
                    (probability, if next() % 16 == 0 { 1 - likely } else { likely })
                })
                .collect();
            let mut encoder = BinaryRangeEncoder::new();
            for &(probability, bit) in &decisions {
                encoder.encode_bit(probability, bit);
            }
            let mut bit_writer = BitWriter::new();
            let written = encoder.finish_into(&mut bit_writer);
            // Whatever follows the body: random bits, read back after it.
            let following: Vec<u64> = (0..100).map(|_| next() & 1).collect();
            for &bit in &following {
                bit_writer.write_bit(bit);
            }
            let bytes = bit_writer.into_bytes();
            let mut bit_reader = BitReader::new(&bytes);
            let mut decoder = BinaryRangeDecoder::start(&mut bit_reader);
            for (index, &(probability, bit)) in decisions.iter().enumerate() {
                assert_eq!(decoder.decode_bit(probability, &mut bit_reader), bit, "trial {trial} decision {index}");
            }
            decoder.finish(&mut bit_reader);
            assert_eq!(bit_reader.bits_read(), written, "trial {trial}: the reader stops where the body ends");
            let read_back: Vec<u64> = (0..100).map(|_| bit_reader.read_bit()).collect();
            assert_eq!(read_back, following, "trial {trial}: what follows the body reads back");
        }
    }
}
