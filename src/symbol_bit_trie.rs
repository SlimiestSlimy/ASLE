//! Every symbol ever defined, by its bits: a binary trie, one node per
//! distinct prefix of a defined symbol's bits; a node whose path spells a
//! symbol holds its id. Symbols are never undefined, so it only grows.
//!
//! What it is for: respelling a leaf (change_prediction) needs every symbol
//! that starts at each of the leaf's bits. Walking the trie along the leaf's
//! bits meets all of them in one pass, shortest first, instead of hashing
//! every substring (MINE, 2026-11-11: the substring lookups were 5–10% of
//! the build and most of its 30–35% in leaf removal checks).

use crate::packed_bit_sequence::PackedBitSequence;
use crate::symbol_definition::SymbolDefinition;

const NO_NODE: u32 = u32::MAX;
const NO_SYMBOL: u32 = u32::MAX;
const ROOT: usize = 0;

#[derive(Clone)]
pub struct SymbolBitTrie {
    /// child_node_by_bit[node] = [the node after a 0, the node after a 1].
    child_node_by_bit: Vec<[u32; 2]>,
    /// The symbol whose bits are the path to the node, if any.
    symbol_id_at_node: Vec<u32>,
}

impl SymbolBitTrie {
    pub fn new() -> Self {
        SymbolBitTrie { child_node_by_bit: vec![[NO_NODE; 2]], symbol_id_at_node: vec![NO_SYMBOL] }
    }

    /// Adds a symbol; its bits must not spell a symbol already in the trie.
    pub fn insert(&mut self, bits: &PackedBitSequence, definition: SymbolDefinition, symbol_id: u32) {
        let mut node = ROOT;
        for bit_index in 0..definition.bit_length as u64 {
            let bit = bits.read_bit(definition.first_bit_offset + bit_index) as usize;
            if self.child_node_by_bit[node][bit] == NO_NODE {
                self.child_node_by_bit[node][bit] = self.child_node_by_bit.len() as u32;
                self.child_node_by_bit.push([NO_NODE; 2]);
                self.symbol_id_at_node.push(NO_SYMBOL);
            }
            node = self.child_node_by_bit[node][bit] as usize;
        }
        debug_assert_eq!(self.symbol_id_at_node[node], NO_SYMBOL, "each symbol's bits are defined once");
        self.symbol_id_at_node[node] = symbol_id;
    }

    /// Every symbol whose bits are the first bits of `bits[first_bit_offset..]`
    /// (at most `most_bits` of them), shortest first: visit(bit length, symbol id).
    pub fn for_each_symbol_starting_at(
        &self,
        bits: &PackedBitSequence,
        first_bit_offset: u64,
        most_bits: u32,
        mut visit: impl FnMut(u32, u32),
    ) {
        let mut node = ROOT;
        for bit_length in 1..=most_bits {
            let bit = bits.read_bit(first_bit_offset + (bit_length - 1) as u64) as usize;
            let child = self.child_node_by_bit[node][bit];
            if child == NO_NODE {
                return;
            }
            node = child as usize;
            if self.symbol_id_at_node[node] != NO_SYMBOL {
                visit(bit_length, self.symbol_id_at_node[node]);
            }
        }
    }
}
