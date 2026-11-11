//! The body: every placed symbol's canonical Huffman code, range coded bit by
//! bit down the code tree (the user's design, 2026-11-10).
//!
//! The context is the last 3 placed symbols (their depth-first positions;
//! START_MARKER stands in before the first ones: MINE). Every (context, code
//! tree node) remembers the last choice taken there: the remembered side gets
//! probability 7/8, the other 1/8; with nothing remembered yet, 1/2 each
//! (the Huffman tree's own split). After each choice the memory is updated.
//! Encoder and decoder keep the same memory, so nothing is stored for it.

use crate::binary_range_coder::{BinaryRangeDecoder, BinaryRangeEncoder, Probability, PROBABILITY_HALF};
use crate::bit_io::{BitReader, BitWriter};
use crate::canonical_huffman_code::CanonicalHuffmanDecoder;
use crate::fast_hash::FastHashMap;

/// The user's choice: the remembered side of a node is predicted 7/8 to 1/8
/// (exact in 2^32: 7 * 2^29; 3/4 and 2/3 measured first).
const REMEMBERED_SIDE_PROBABILITY: Probability = 7 << 29;
const START_MARKER: u32 = u32::MAX;

/// The last 3 placed symbols' positions, oldest first.
type Order3Context = [u32; 3];
/// A code tree node: (depth, the code bits on the way to it).
type CodeTreeNode = (u32, u64);

#[derive(Default)]
struct LastChoiceMemory {
    last_choice: FastHashMap<(Order3Context, CodeTreeNode), u8>,
}

impl LastChoiceMemory {
    fn probability_of_zero(&self, context: Order3Context, node: CodeTreeNode) -> Probability {
        match self.last_choice.get(&(context, node)) {
            None => PROBABILITY_HALF,
            Some(0) => REMEMBERED_SIDE_PROBABILITY,
            // 2^32 - p, as a u32: the other side.
            Some(_) => REMEMBERED_SIDE_PROBABILITY.wrapping_neg(),
        }
    }

    fn remember(&mut self, context: Order3Context, node: CodeTreeNode, choice: u64) {
        self.last_choice.insert((context, node), choice as u8);
    }
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
    let mut context: Order3Context = [START_MARKER; 3];
    for position in placed_positions {
        let (code_bits, code_length) = codes_in_order[position];
        for depth in 0..code_length {
            let node: CodeTreeNode = (depth, code_bits >> (code_length - depth));
            let choice = (code_bits >> (code_length - 1 - depth)) & 1;
            encoder.encode_bit(memory.probability_of_zero(context, node), choice);
            memory.remember(context, node, choice);
        }
        context = advance_context(context, position);
    }
    encoder.finish_into(bit_writer)
}

/// Reads `placed_symbol_count` symbols; returns their depth-first positions.
pub fn decode_body(
    bit_reader: &mut BitReader,
    placed_symbol_count: u64,
    code_decoder: &CanonicalHuffmanDecoder,
) -> Vec<usize> {
    let mut decoder = BinaryRangeDecoder::start(bit_reader);
    let mut memory = LastChoiceMemory::default();
    let mut context: Order3Context = [START_MARKER; 3];
    let mut placed_positions = Vec::with_capacity(placed_symbol_count as usize);
    for _ in 0..placed_symbol_count {
        let (mut code_bits, mut code_length) = (0u64, 0u32);
        let position = loop {
            let node: CodeTreeNode = (code_length, code_bits);
            let choice = decoder.decode_bit(memory.probability_of_zero(context, node), bit_reader);
            memory.remember(context, node, choice);
            code_bits = (code_bits << 1) | choice;
            code_length += 1;
            if let Some(position) = code_decoder.position_of_code(code_bits, code_length) {
                break position;
            }
        };
        placed_positions.push(position);
        context = advance_context(context, position);
    }
    placed_positions
}
