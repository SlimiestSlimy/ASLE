//! The body: every placed symbol's canonical Huffman code, range coded bit by
//! bit down the code tree (the user's design, 2026-11-10).
//!
//! The context is the last 3 placed symbols (their depth-first positions;
//! START_MARKER stands in before the first ones: MINE). Every (context, code
//! tree node) remembers the last choice taken there. With nothing remembered
//! yet, each side gets 1/2 (the Huffman tree's own split). Otherwise the
//! remembered side gets the body's success rate so far: one global pair of
//! counters, remembered side right / wrong, over every node that had a
//! remembered side (the user's design, 2026-11-11, replacing a fixed 7/8).
//! When the memory keeps being right, a right guess becomes very cheap; when
//! it is right half the time, the split drifts back to 1/2.
//!
//! The counters learn as the tiler's walk does (the user's local decay): the
//! outcome's counter gains 256, then each counter loses its share
//! right * wrong / (right + wrong)^2 of itself, rounded down. So a lopsided
//! pair barely decays, and a mixed one forgets fast.
//!
//! After each choice the memory and the counters are updated. Encoder and
//! decoder keep the same ones, so nothing is stored for them. The coder runs
//! unbroken through the whole body and outputs a single bit whenever its
//! range falls below 2^31 (binary_range_coder): precision shaped, not symbol
//! or byte shaped.

use crate::binary_range_coder::{
    BinaryRangeDecoder, BinaryRangeEncoder, Probability, LEAST_PROBABILITY_OF_A_SIDE, PROBABILITY_HALF,
};
use crate::bit_io::{BitReader, BitWriter};
use crate::canonical_huffman_code::CanonicalHuffmanDecoder;
use crate::fast_hash::FastHashMap;

/// What one outcome adds to its counter (the tiler's step: "+256 equals floats").
const SUCCESS_COUNTER_STEP: u64 = 256;
const START_MARKER: u32 = u32::MAX;

/// The last 3 placed symbols' positions, oldest first.
type Order3Context = [u32; 3];
/// A code tree node: (depth, the code bits on the way to it).
type CodeTreeNode = (u32, u64);

#[derive(Default)]
struct LastChoiceMemory {
    last_choice: FastHashMap<(Order3Context, CodeTreeNode), u8>,
}

/// How often the remembered side has been the one taken, over the whole body.
#[derive(Default)]
struct RememberedSideSuccessRate {
    remembered_side_taken: u64,
    other_side_taken: u64,
}

impl RememberedSideSuccessRate {
    /// right / (right + wrong) in 2^32 fractions, rounded down; 1/2 before any
    /// outcome. Kept within what the coder can split (MINE): at least
    /// LEAST_PROBABILITY_OF_A_SIDE for either side.
    fn probability_of_remembered_side(&self) -> u64 {
        let outcomes = self.remembered_side_taken + self.other_side_taken;
        if outcomes == 0 {
            return PROBABILITY_HALF as u64;
        }
        let probability = ((self.remembered_side_taken as u128) << 32) / outcomes as u128;
        (probability as u64).clamp(LEAST_PROBABILITY_OF_A_SIDE, (1 << 32) - LEAST_PROBABILITY_OF_A_SIDE)
    }

    /// +256 to the outcome's counter, then the local decay.
    fn record(&mut self, remembered_side_was_taken: bool) {
        if remembered_side_was_taken {
            self.remembered_side_taken += SUCCESS_COUNTER_STEP;
        } else {
            self.other_side_taken += SUCCESS_COUNTER_STEP;
        }
        let (right, wrong) = (self.remembered_side_taken as u128, self.other_side_taken as u128);
        let outcomes_squared = (right + wrong) * (right + wrong);
        self.remembered_side_taken -= (right * right * wrong / outcomes_squared) as u64;
        self.other_side_taken -= (right * wrong * wrong / outcomes_squared) as u64;
    }
}

impl LastChoiceMemory {
    fn remembered_choice(&self, context: Order3Context, node: CodeTreeNode) -> Option<u8> {
        self.last_choice.get(&(context, node)).copied()
    }

    fn remember(&mut self, context: Order3Context, node: CodeTreeNode, choice: u64) {
        self.last_choice.insert((context, node), choice as u8);
    }
}

/// The node's probability of a 0: 1/2 with nothing remembered, otherwise the
/// success rate on the remembered side.
fn probability_of_zero(remembered_choice: Option<u8>, success_rate: &RememberedSideSuccessRate) -> Probability {
    match remembered_choice {
        None => PROBABILITY_HALF,
        Some(0) => success_rate.probability_of_remembered_side() as Probability,
        Some(_) => ((1u64 << 32) - success_rate.probability_of_remembered_side()) as Probability,
    }
}

/// After a choice: the success rate learns (if a side was remembered), then
/// the memory takes the choice.
fn learn_choice(
    memory: &mut LastChoiceMemory,
    success_rate: &mut RememberedSideSuccessRate,
    remembered_choice: Option<u8>,
    context: Order3Context,
    node: CodeTreeNode,
    choice: u64,
) {
    if let Some(remembered) = remembered_choice {
        success_rate.record(remembered as u64 == choice);
    }
    memory.remember(context, node, choice);
}

fn advance_context(context: Order3Context, placed_position: usize) -> Order3Context {
    [context[1], context[2], placed_position as u32]
}

/// Writes the body; returns its bits. `codes_in_order[position]` is the
/// canonical (code bits, code length) of the symbol at that depth-first position.
pub fn encode_body(
    bit_writer: &mut BitWriter,
    placed_positions: impl Iterator<Item = usize>,
    codes_in_order: &[(u64, u32)],
) -> u64 {
    let mut encoder = BinaryRangeEncoder::new();
    let mut memory = LastChoiceMemory::default();
    let mut success_rate = RememberedSideSuccessRate::default();
    let mut context: Order3Context = [START_MARKER; 3];
    for position in placed_positions {
        let (code_bits, code_length) = codes_in_order[position];
        for depth in 0..code_length {
            let node: CodeTreeNode = (depth, code_bits >> (code_length - depth));
            let choice = (code_bits >> (code_length - 1 - depth)) & 1;
            let remembered_choice = memory.remembered_choice(context, node);
            encoder.encode_bit(probability_of_zero(remembered_choice, &success_rate), choice);
            learn_choice(&mut memory, &mut success_rate, remembered_choice, context, node, choice);
        }
        context = advance_context(context, position);
    }
    encoder.finish_into(bit_writer)
}

/// Reads `placed_symbol_count` symbols; returns their depth-first positions
/// and leaves `bit_reader` right after the body.
pub fn decode_body(
    bit_reader: &mut BitReader,
    placed_symbol_count: u64,
    code_decoder: &CanonicalHuffmanDecoder,
) -> Vec<usize> {
    let mut decoder = BinaryRangeDecoder::start(bit_reader);
    let mut memory = LastChoiceMemory::default();
    let mut success_rate = RememberedSideSuccessRate::default();
    let mut context: Order3Context = [START_MARKER; 3];
    let mut placed_positions = Vec::with_capacity(placed_symbol_count as usize);
    for _ in 0..placed_symbol_count {
        let (mut code_bits, mut code_length) = (0u64, 0u32);
        let position = loop {
            let node: CodeTreeNode = (code_length, code_bits);
            let remembered_choice = memory.remembered_choice(context, node);
            let choice = decoder.decode_bit(probability_of_zero(remembered_choice, &success_rate), bit_reader);
            learn_choice(&mut memory, &mut success_rate, remembered_choice, context, node, choice);
            code_bits = (code_bits << 1) | choice;
            code_length += 1;
            if let Some(position) = code_decoder.position_of_code(code_bits, code_length) {
                break position;
            }
        };
        placed_positions.push(position);
        context = advance_context(context, position);
    }
    decoder.finish(bit_reader);
    placed_positions
}
