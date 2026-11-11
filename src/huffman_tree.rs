//! Huffman tree: exact weight (W), code lengths, construction, depth-first
//! serialization and decoding.
//!
//! W ("huffman weight in bits") = sum over all leaves of symbol frequency *
//! leaf depth = the exact number of bits the encoded symbols take up.

use crate::bit_io::{BitReader, BitWriter};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// W: sum over leaves of frequency * depth, computed as the sum of all merged
/// node weights while building the tree. A lone symbol still costs 1 bit per
/// occurrence.
pub fn calculate_huffman_weight_in_bits(symbol_frequencies: &[u64]) -> u64 {
    let mut lightest_first: BinaryHeap<Reverse<u64>> =
        symbol_frequencies.iter().copied().filter(|&frequency| frequency > 0).map(Reverse).collect();
    if lightest_first.len() == 1 {
        return lightest_first.pop().unwrap().0;
    }
    let mut huffman_weight_in_bits = 0;
    while lightest_first.len() > 1 {
        let Reverse(lightest_weight) = lightest_first.pop().unwrap();
        let Reverse(second_lightest_weight) = lightest_first.pop().unwrap();
        huffman_weight_in_bits += lightest_weight + second_lightest_weight;
        lightest_first.push(Reverse(lightest_weight + second_lightest_weight));
    }
    huffman_weight_in_bits
}

/// Code length (leaf depth) of every symbol; 0 for symbols with frequency 0.
pub fn calculate_code_lengths(symbol_frequencies: &[u64]) -> Vec<u32> {
    if symbol_frequencies.iter().all(|&frequency| frequency == 0) {
        return vec![0; symbol_frequencies.len()];
    }
    HuffmanTree::build_from_frequencies(symbol_frequencies)
        .calculate_codes_by_symbol_id(symbol_frequencies.len())
        .iter()
        .map(|&(_, code_length)| code_length)
        .collect()
}

#[derive(Clone, Copy)]
pub enum HuffmanNode {
    Leaf { symbol_id: usize },
    Internal { left_child_node_id: usize, right_child_node_id: usize },
}

pub struct HuffmanTree {
    pub nodes: Vec<HuffmanNode>,
    pub root_node_id: usize,
}

impl HuffmanTree {
    /// Requires at least one nonzero frequency. Ties are broken by node id, so
    /// the tree is deterministic.
    pub fn build_from_frequencies(symbol_frequencies: &[u64]) -> HuffmanTree {
        let mut nodes = Vec::new();
        let mut lightest_first = BinaryHeap::new();
        for (symbol_id, &frequency) in symbol_frequencies.iter().enumerate() {
            if frequency > 0 {
                nodes.push(HuffmanNode::Leaf { symbol_id });
                lightest_first.push(Reverse((frequency, nodes.len() - 1)));
            }
        }
        assert!(!nodes.is_empty(), "a Huffman tree needs at least one symbol");
        if nodes.len() == 1 {
            // A single symbol still needs a 1-bit code: a root with the leaf on both sides.
            nodes.push(HuffmanNode::Internal { left_child_node_id: 0, right_child_node_id: 0 });
            return HuffmanTree { root_node_id: 1, nodes };
        }
        while lightest_first.len() > 1 {
            let Reverse((lightest_weight, lightest_node_id)) = lightest_first.pop().unwrap();
            let Reverse((second_lightest_weight, second_lightest_node_id)) = lightest_first.pop().unwrap();
            nodes.push(HuffmanNode::Internal {
                left_child_node_id: lightest_node_id,
                right_child_node_id: second_lightest_node_id,
            });
            lightest_first.push(Reverse((lightest_weight + second_lightest_weight, nodes.len() - 1)));
        }
        let root_node_id = lightest_first.pop().unwrap().0 .1;
        HuffmanTree { nodes, root_node_id }
    }

    /// (code bits, code length) for every symbol id; left child = 0, right child = 1.
    pub fn calculate_codes_by_symbol_id(&self, symbol_count: usize) -> Vec<(u64, u32)> {
        let mut codes_by_symbol_id = vec![(0u64, 0u32); symbol_count];
        let mut symbol_already_coded = vec![false; symbol_count];
        let mut nodes_to_visit = vec![(self.root_node_id, 0u64, 0u32)];
        while let Some((node_id, code_bits, code_length)) = nodes_to_visit.pop() {
            match self.nodes[node_id] {
                HuffmanNode::Leaf { symbol_id } => {
                    if !symbol_already_coded[symbol_id] {
                        symbol_already_coded[symbol_id] = true;
                        codes_by_symbol_id[symbol_id] = (code_bits, code_length);
                    }
                }
                HuffmanNode::Internal { left_child_node_id, right_child_node_id } => {
                    assert!(code_length < 64, "Huffman code longer than 64 bits");
                    nodes_to_visit.push((right_child_node_id, (code_bits << 1) | 1, code_length + 1));
                    nodes_to_visit.push((left_child_node_id, code_bits << 1, code_length + 1));
                }
            }
        }
        codes_by_symbol_id
    }

    /// Depth-first: bit 1 = internal node (then left subtree, right subtree),
    /// bit 0 = leaf (then whatever `write_leaf_payload` writes for that symbol).
    pub fn write_depth_first(
        &self,
        bit_writer: &mut BitWriter,
        write_leaf_payload: &mut impl FnMut(&mut BitWriter, usize),
    ) {
        self.write_node_depth_first(self.root_node_id, bit_writer, write_leaf_payload);
    }

    fn write_node_depth_first(
        &self,
        node_id: usize,
        bit_writer: &mut BitWriter,
        write_leaf_payload: &mut impl FnMut(&mut BitWriter, usize),
    ) {
        match self.nodes[node_id] {
            HuffmanNode::Leaf { symbol_id } => {
                bit_writer.write_bit(0);
                write_leaf_payload(bit_writer, symbol_id);
            }
            HuffmanNode::Internal { left_child_node_id, right_child_node_id } => {
                bit_writer.write_bit(1);
                self.write_node_depth_first(left_child_node_id, bit_writer, write_leaf_payload);
                self.write_node_depth_first(right_child_node_id, bit_writer, write_leaf_payload);
            }
        }
    }

    /// Inverse of `write_depth_first`; `read_leaf_payload` reads one leaf's
    /// payload and returns the symbol id it stands for.
    pub fn read_depth_first(
        bit_reader: &mut BitReader,
        read_leaf_payload: &mut impl FnMut(&mut BitReader) -> usize,
    ) -> HuffmanTree {
        let mut nodes = Vec::new();
        let root_node_id = Self::read_node_depth_first(bit_reader, read_leaf_payload, &mut nodes);
        HuffmanTree { nodes, root_node_id }
    }

    fn read_node_depth_first(
        bit_reader: &mut BitReader,
        read_leaf_payload: &mut impl FnMut(&mut BitReader) -> usize,
        nodes: &mut Vec<HuffmanNode>,
    ) -> usize {
        if bit_reader.read_bit() == 0 {
            let symbol_id = read_leaf_payload(bit_reader);
            nodes.push(HuffmanNode::Leaf { symbol_id });
        } else {
            let left_child_node_id = Self::read_node_depth_first(bit_reader, read_leaf_payload, nodes);
            let right_child_node_id = Self::read_node_depth_first(bit_reader, read_leaf_payload, nodes);
            nodes.push(HuffmanNode::Internal { left_child_node_id, right_child_node_id });
        }
        nodes.len() - 1
    }

    /// Walks from the root following bits until a leaf; returns its symbol id.
    pub fn decode_next_symbol_id(&self, bit_reader: &mut BitReader) -> usize {
        let mut node_id = self.root_node_id;
        loop {
            match self.nodes[node_id] {
                HuffmanNode::Leaf { symbol_id } => return symbol_id,
                HuffmanNode::Internal { left_child_node_id, right_child_node_id } => {
                    node_id = if bit_reader.read_bit() == 0 { left_child_node_id } else { right_child_node_id };
                }
            }
        }
    }
}
