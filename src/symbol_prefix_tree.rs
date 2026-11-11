//! The symbol set, stored as a binary prefix tree of the symbols' bits.
//!
//! Layout, everything depth first:
//! * skipped full levels: 4 bits. Every node shallower than this has both
//!   children, so their child bits are not written.
//! * child bits: for every node at or below the skipped levels, 2 bits
//!   (has a 0-child, has a 1-child).
//! * Elias gamma(k + 1), k = how many nodes with children are symbols
//!   themselves (the root never is, so it is left out).
//! * combination index: which k of the m non-root nodes with children (in
//!   depth-first order) are symbols, as a number in [0, C(m, k)) written in
//!   exactly enough bits for C(m, k) - 1.
//! Nodes without children are always symbols.
//!
//! Symbols come out in depth-first order: bit by bit, 0 before 1, a prefix
//! before its extensions. The code tree's leaves follow the same order.

use crate::big_unsigned_integer::{binomial_coefficient, BigUnsignedInteger};
use crate::bit_io::{BitReader, BitWriter};
use crate::packed_bit_sequence::PackedBitSequence;
use crate::symbol_definition::SymbolDefinition;

/// Width of the skipped-levels field; at most 15 levels can be skipped.
const SKIPPED_LEVELS_FIELD_BITS: u32 = 4;
const MOST_SKIPPABLE_LEVELS: u32 = (1 << SKIPPED_LEVELS_FIELD_BITS) - 1;

#[derive(Clone, Copy)]
struct PrefixTreeNode {
    depth: u32,
    has_zero_child: bool,
    has_one_child: bool,
    is_symbol: bool,
}

impl PrefixTreeNode {
    fn has_children(&self) -> bool {
        self.has_zero_child || self.has_one_child
    }
}

/// The encoder's description of the prefix tree, from which both the exact
/// bit count and the written bits are derived.
pub struct SymbolPrefixTreeDescription {
    nodes_depth_first: Vec<PrefixTreeNode>,
    skipped_full_levels: u32,
    /// The symbols in depth-first order: the order the decoder lists them in.
    pub symbol_ids_depth_first: Vec<usize>,
}

/// Describes the prefix tree of the given symbols (each with its id).
pub fn describe_symbol_prefix_tree(
    shared_bit_sequence: &PackedBitSequence,
    mut symbols: Vec<(usize, SymbolDefinition)>,
) -> SymbolPrefixTreeDescription {
    symbols.sort_by(|(_, first), (_, second)| {
        shared_bit_sequence.compare_slices_depth_first(
            first.first_bit_offset,
            first.bit_length as u64,
            second.first_bit_offset,
            second.bit_length as u64,
        )
    });
    let mut nodes_depth_first = Vec::new();
    describe_node(shared_bit_sequence, &symbols, 0, &mut nodes_depth_first);
    let shallowest_depth_missing_a_child = nodes_depth_first
        .iter()
        .filter(|node| !(node.has_zero_child && node.has_one_child))
        .map(|node| node.depth)
        .min()
        .unwrap_or(0);
    SymbolPrefixTreeDescription {
        nodes_depth_first,
        skipped_full_levels: shallowest_depth_missing_a_child.min(MOST_SKIPPABLE_LEVELS),
        symbol_ids_depth_first: symbols.iter().map(|&(symbol_id, _)| symbol_id).collect(),
    }
}

/// `run`: the sorted symbols whose first `depth` bits are this node's path.
fn describe_node(
    shared_bit_sequence: &PackedBitSequence,
    run: &[(usize, SymbolDefinition)],
    depth: u32,
    nodes_depth_first: &mut Vec<PrefixTreeNode>,
) {
    // Sorted with prefixes first, so a symbol ending exactly here comes first.
    let is_symbol = depth > 0 && run.first().map_or(false, |(_, definition)| definition.bit_length == depth);
    let below = if is_symbol { &run[1..] } else { run };
    let bit_at_depth = |definition: &SymbolDefinition| shared_bit_sequence.read_bit(definition.first_bit_offset + depth as u64);
    let first_with_one = below.iter().position(|(_, definition)| bit_at_depth(definition) == 1).unwrap_or(below.len());
    let (zero_run, one_run) = below.split_at(first_with_one);
    nodes_depth_first.push(PrefixTreeNode {
        depth,
        has_zero_child: !zero_run.is_empty(),
        has_one_child: !one_run.is_empty(),
        is_symbol,
    });
    if !zero_run.is_empty() {
        describe_node(shared_bit_sequence, zero_run, depth + 1, nodes_depth_first);
    }
    if !one_run.is_empty() {
        describe_node(shared_bit_sequence, one_run, depth + 1, nodes_depth_first);
    }
}

fn elias_gamma_bit_count(value: u64) -> u64 {
    2 * (64 - value.leading_zeros() as u64) - 1
}

/// Non-root nodes with children, in depth-first order: whether each is a symbol.
fn symbol_flags_of_nodes_with_children(nodes_depth_first: &[PrefixTreeNode]) -> Vec<bool> {
    nodes_depth_first.iter().filter(|node| node.depth > 0 && node.has_children()).map(|node| node.is_symbol).collect()
}

/// Exact bits to index any of C(m, k) combinations.
fn combination_index_bit_count(nodes_with_children: u64, symbols_among_them: u64) -> u64 {
    let mut largest_index = binomial_coefficient(nodes_with_children, symbols_among_them);
    largest_index.subtract_assign(&BigUnsignedInteger::from_u64(1));
    largest_index.bit_length()
}

/// Rank of the chosen positions among all C(m, k) choices (combinatorial
/// number system): sum over the j-th chosen position c (counting from 1) of C(c, j).
fn rank_combination(flags: &[bool]) -> BigUnsignedInteger {
    let mut rank = BigUnsignedInteger::zero();
    let mut chosen_so_far = 0u64;
    for (position, &chosen) in flags.iter().enumerate() {
        if chosen {
            chosen_so_far += 1;
            rank.add_assign(&binomial_coefficient(position as u64, chosen_so_far));
        }
    }
    rank
}

/// Inverse of `rank_combination`.
fn unrank_combination(mut rank: BigUnsignedInteger, position_count: u64, chosen_count: u64) -> Vec<bool> {
    let mut flags = vec![false; position_count as usize];
    let mut upper_bound = position_count;
    for chosen_number in (1..=chosen_count).rev() {
        // The largest position below upper_bound with C(position, chosen_number) <= rank.
        let mut position = upper_bound - 1;
        let mut value = binomial_coefficient(position, chosen_number);
        while value.compare(&rank) == std::cmp::Ordering::Greater {
            // C(position - 1, j) = C(position, j) * (position - j) / position
            value.multiply_by_small(position - chosen_number);
            value.divide_by_small(position);
            position -= 1;
        }
        flags[position as usize] = true;
        rank.subtract_assign(&value);
        upper_bound = position;
    }
    flags
}

impl SymbolPrefixTreeDescription {
    pub fn bit_count(&self) -> u64 {
        let child_bits = 2 * self.nodes_depth_first.iter().filter(|node| node.depth >= self.skipped_full_levels).count() as u64;
        let flags = symbol_flags_of_nodes_with_children(&self.nodes_depth_first);
        let symbols_with_children = flags.iter().filter(|&&flag| flag).count() as u64;
        SKIPPED_LEVELS_FIELD_BITS as u64
            + child_bits
            + elias_gamma_bit_count(symbols_with_children + 1)
            + combination_index_bit_count(flags.len() as u64, symbols_with_children)
    }

    pub fn write(&self, bit_writer: &mut BitWriter) {
        bit_writer.write_bits(self.skipped_full_levels as u64, SKIPPED_LEVELS_FIELD_BITS);
        for node in self.nodes_depth_first.iter().filter(|node| node.depth >= self.skipped_full_levels) {
            bit_writer.write_bit(node.has_zero_child as u64);
            bit_writer.write_bit(node.has_one_child as u64);
        }
        let flags = symbol_flags_of_nodes_with_children(&self.nodes_depth_first);
        let symbols_with_children = flags.iter().filter(|&&flag| flag).count() as u64;
        bit_writer.write_elias_gamma(symbols_with_children + 1);
        let index_bits = combination_index_bit_count(flags.len() as u64, symbols_with_children);
        rank_combination(&flags).write_bits(bit_writer, index_bits);
    }
}

/// Reads the prefix tree; returns the symbols' bits (appended into a new
/// shared sequence) and their definitions, in depth-first order.
pub fn read_symbol_prefix_tree(bit_reader: &mut BitReader) -> (PackedBitSequence, Vec<SymbolDefinition>) {
    let skipped_full_levels = bit_reader.read_bits(SKIPPED_LEVELS_FIELD_BITS) as u32;
    let mut nodes_depth_first = Vec::new();
    read_node_shape(bit_reader, 0, skipped_full_levels, &mut nodes_depth_first);

    let node_indexes_with_children: Vec<usize> = (0..nodes_depth_first.len())
        .filter(|&index| nodes_depth_first[index].depth > 0 && nodes_depth_first[index].has_children())
        .collect();
    let symbols_with_children = bit_reader.read_elias_gamma() - 1;
    let index_bits = combination_index_bit_count(node_indexes_with_children.len() as u64, symbols_with_children);
    let rank = BigUnsignedInteger::read_bits(bit_reader, index_bits);
    let flags = unrank_combination(rank, node_indexes_with_children.len() as u64, symbols_with_children);
    for (&node_index, &is_symbol) in node_indexes_with_children.iter().zip(&flags) {
        nodes_depth_first[node_index].is_symbol = is_symbol;
    }
    for node in nodes_depth_first.iter_mut() {
        if node.depth > 0 && !node.has_children() {
            node.is_symbol = true;
        }
    }

    let mut decoded_symbol_bits = PackedBitSequence::new();
    let mut symbol_definitions = Vec::new();
    let mut path_bits: Vec<u64> = Vec::new();
    let mut next_node_index = 0;
    emit_symbols_depth_first(
        &nodes_depth_first,
        &mut next_node_index,
        &mut path_bits,
        &mut decoded_symbol_bits,
        &mut symbol_definitions,
    );
    (decoded_symbol_bits, symbol_definitions)
}

fn read_node_shape(bit_reader: &mut BitReader, depth: u32, skipped_full_levels: u32, nodes: &mut Vec<PrefixTreeNode>) {
    let (has_zero_child, has_one_child) = if depth < skipped_full_levels {
        (true, true)
    } else {
        (bit_reader.read_bit() == 1, bit_reader.read_bit() == 1)
    };
    nodes.push(PrefixTreeNode { depth, has_zero_child, has_one_child, is_symbol: false });
    if has_zero_child {
        read_node_shape(bit_reader, depth + 1, skipped_full_levels, nodes);
    }
    if has_one_child {
        read_node_shape(bit_reader, depth + 1, skipped_full_levels, nodes);
    }
}

fn emit_symbols_depth_first(
    nodes_depth_first: &[PrefixTreeNode],
    next_node_index: &mut usize,
    path_bits: &mut Vec<u64>,
    decoded_symbol_bits: &mut PackedBitSequence,
    symbol_definitions: &mut Vec<SymbolDefinition>,
) {
    let node = nodes_depth_first[*next_node_index];
    *next_node_index += 1;
    if node.is_symbol {
        let first_bit_offset = decoded_symbol_bits.bit_count();
        for &bit in path_bits.iter() {
            decoded_symbol_bits.push_bit(bit);
        }
        symbol_definitions.push(SymbolDefinition { first_bit_offset, bit_length: path_bits.len() as u32 });
    }
    for (child_bit, has_child) in [(0u64, node.has_zero_child), (1u64, node.has_one_child)] {
        if has_child {
            path_bits.push(child_bit);
            emit_symbols_depth_first(nodes_depth_first, next_node_index, path_bits, decoded_symbol_bits, symbol_definitions);
            path_bits.pop();
        }
    }
}
