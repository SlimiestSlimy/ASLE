//! The parse state: the input as a sequence of placed symbols, every symbol
//! definition ever created, and how often each symbol is placed (its
//! frequency; a symbol with frequency > 0 is a leaf of the Huffman tree).
//!
//! Symbols are slices of one shared bit sequence: the input bits, followed by
//! one "0" and one "1" so the two starting symbols always have bits to point at.
//!
//! This module is the application stage: it is the only code that changes the
//! state. Everything else reads it.

use crate::fast_hash::FastHashMap;
use crate::huffman_tree::calculate_code_lengths;
use crate::leaf_counts_by_weight::LeafCountsByWeight;
use crate::packed_bit_sequence::PackedBitSequence;
use crate::predicted_changes::{FrequencyChange, LeafRemovalPrediction, MergePrediction};
use crate::symbol_definition::SymbolDefinition;

/// Marks a position whose symbol was absorbed into a merge earlier in the
/// current pass; removed by `remove_vacated_positions`.
pub const VACATED_POSITION: u32 = u32::MAX;

#[derive(Clone)]
pub struct SymbolStream {
    pub shared_bit_sequence: PackedBitSequence,
    pub symbol_definitions: Vec<SymbolDefinition>,
    pub symbol_frequencies: Vec<u64>,
    /// How many leaves have each frequency, changed only together with
    /// `symbol_frequencies` (apply_frequency_changes); the builder tries
    /// candidates on working copies of it.
    leaf_counts_by_weight: LeafCountsByWeight,
    /// The input spelled as placed symbols, in order; their bits are
    /// consecutive input bits, so a position's input offset is the sum of the
    /// bit lengths before it.
    pub placed_symbol_ids: Vec<u32>,
    /// n: the largest window size, and so the longest possible symbol.
    pub maximum_symbol_bit_length: u32,
    pub input_bit_count: u64,
    symbol_ids_by_content_hash: FastHashMap<u64, Vec<u32>>,
}

/// Number of leaves: symbols with a nonzero frequency.
pub fn count_leaves(symbol_frequencies: &[u64]) -> usize {
    symbol_frequencies.iter().filter(|&&frequency| frequency > 0).count()
}

impl SymbolStream {
    /// Every input bit (the first `input_bit_count` bits of the bytes) becomes
    /// a placed "0" or "1" symbol.
    pub fn from_input_bits(input_bytes: &[u8], input_bit_count: u64, maximum_symbol_bit_length: u32) -> Self {
        assert!(maximum_symbol_bit_length >= 2, "maximum symbol bit length must be at least 2");
        let mut shared_bit_sequence = PackedBitSequence::from_first_bits_of_bytes(input_bytes, input_bit_count);
        let input_bit_count = shared_bit_sequence.bit_count();
        let mut placed_symbol_ids = Vec::with_capacity(input_bit_count as usize);
        for bit_offset in 0..input_bit_count {
            placed_symbol_ids.push(shared_bit_sequence.read_bit(bit_offset) as u32);
        }
        shared_bit_sequence.push_bit(0);
        shared_bit_sequence.push_bit(1);
        let one_count = placed_symbol_ids.iter().filter(|&&symbol_id| symbol_id == 1).count() as u64;
        let zero_count = input_bit_count - one_count;
        let mut stream = SymbolStream {
            shared_bit_sequence,
            symbol_definitions: Vec::new(),
            symbol_frequencies: Vec::new(),
            leaf_counts_by_weight: LeafCountsByWeight::default(),
            placed_symbol_ids,
            maximum_symbol_bit_length,
            input_bit_count,
            symbol_ids_by_content_hash: FastHashMap::default(),
        };
        stream.add_symbol_definition(SymbolDefinition { first_bit_offset: input_bit_count, bit_length: 1 });
        stream.add_symbol_definition(SymbolDefinition { first_bit_offset: input_bit_count + 1, bit_length: 1 });
        stream.symbol_frequencies = vec![zero_count, one_count];
        stream.leaf_counts_by_weight = LeafCountsByWeight::from_symbol_frequencies(&stream.symbol_frequencies);
        stream
    }

    fn add_symbol_definition(&mut self, definition: SymbolDefinition) {
        let content_hash = self.hash_definition(definition);
        let symbol_id = self.symbol_definitions.len() as u32;
        self.symbol_definitions.push(definition);
        self.symbol_ids_by_content_hash.entry(content_hash).or_default().push(symbol_id);
    }

    pub fn hash_definition(&self, definition: SymbolDefinition) -> u64 {
        self.shared_bit_sequence.hash_slice(definition.first_bit_offset, definition.bit_length as u64)
    }

    pub fn definitions_have_equal_bits(&self, first: SymbolDefinition, second: SymbolDefinition) -> bool {
        first.bit_length == second.bit_length
            && self.shared_bit_sequence.slices_are_equal(
                first.first_bit_offset,
                second.first_bit_offset,
                first.bit_length as u64,
            )
    }

    pub fn leaf_count(&self) -> usize {
        count_leaves(&self.symbol_frequencies)
    }

    /// Exact bits the encoded symbols take up: sum of frequency * code length.
    pub fn encoded_symbol_bits(&self) -> u64 {
        self.leaf_counts_by_weight.encoded_symbol_bits()
    }

    /// The up-to-date leaf counts by weight (read only; they change with the frequencies).
    pub fn leaf_counts_by_weight(&self) -> &LeafCountsByWeight {
        &self.leaf_counts_by_weight
    }

    /// Every symbol's Huffman code length, by symbol id (a heap build).
    pub fn code_lengths_by_symbol_id(&self) -> Vec<u32> {
        let mut code_lengths = calculate_code_lengths(&self.symbol_frequencies);
        code_lengths.resize(self.symbol_definitions.len(), 0);
        code_lengths
    }

    /// The frequency table and the leaf counts change together, here only.
    fn apply_frequency_changes(&mut self, changes: &[FrequencyChange]) {
        self.leaf_counts_by_weight.apply_frequency_changes(changes, &self.symbol_frequencies);
        for change in changes {
            let symbol_id = change.symbol_id as usize;
            if self.symbol_frequencies.len() <= symbol_id {
                self.symbol_frequencies.resize(symbol_id + 1, 0);
            }
            self.symbol_frequencies[symbol_id] = (self.symbol_frequencies[symbol_id] as i64 + change.delta) as u64;
        }
    }

    /// Any symbol ever defined with the same bits, alive (leaf) or dead
    /// (frequency 0). Exact: equal hashes are confirmed bit by bit.
    pub fn find_symbol_id(&self, definition: SymbolDefinition) -> Option<usize> {
        let same_hash_ids = self.symbol_ids_by_content_hash.get(&self.hash_definition(definition))?;
        same_hash_ids
            .iter()
            .map(|&symbol_id| symbol_id as usize)
            .find(|&symbol_id| self.definitions_have_equal_bits(self.symbol_definitions[symbol_id], definition))
    }

    pub fn definition_as_bit_string(&self, definition: SymbolDefinition) -> String {
        self.shared_bit_sequence.slice_as_bit_string(definition.first_bit_offset, definition.bit_length as u64)
    }

    pub fn symbol_as_bit_string(&self, symbol_id: usize) -> String {
        self.definition_as_bit_string(self.symbol_definitions[symbol_id])
    }

    pub fn occupied_position_count(&self) -> usize {
        self.placed_symbol_ids.iter().filter(|&&symbol_id| symbol_id != VACATED_POSITION).count()
    }

    /// Replaces every predicted occurrence with the merged symbol. The first
    /// position of each occurrence holds the merged symbol, the rest are
    /// vacated; all of them are marked consumed for the rest of the pass.
    pub fn apply_merge(&mut self, merge: &MergePrediction, position_consumed_this_pass: &mut [bool]) {
        if merge.merged_symbol_id == self.symbol_definitions.len() {
            self.add_symbol_definition(merge.merged_definition);
        }
        self.apply_frequency_changes(&merge.frequency_changes);
        for &(start_position, end_position) in &merge.merged_occurrence_ranges {
            self.placed_symbol_ids[start_position as usize] = merge.merged_symbol_id as u32;
            position_consumed_this_pass[start_position as usize] = true;
            for position in start_position + 1..end_position {
                self.placed_symbol_ids[position as usize] = VACATED_POSITION;
                position_consumed_this_pass[position as usize] = true;
            }
        }
    }

    /// Replaces every placement of the removed leaf with its respelling and
    /// drops vacated positions. Positions change, so the pass must end.
    pub fn apply_leaf_removal(&mut self, removal: &LeafRemovalPrediction) {
        self.apply_frequency_changes(&removal.frequency_changes);
        let mut rebuilt_symbol_ids = Vec::with_capacity(self.placed_symbol_ids.len());
        for &symbol_id in &self.placed_symbol_ids {
            if symbol_id == VACATED_POSITION {
                continue;
            }
            if symbol_id as usize == removal.removed_symbol_id {
                rebuilt_symbol_ids.extend_from_slice(&removal.replacement_symbol_ids);
            } else {
                rebuilt_symbol_ids.push(symbol_id);
            }
        }
        self.placed_symbol_ids = rebuilt_symbol_ids;
    }

    pub fn remove_vacated_positions(&mut self) {
        self.placed_symbol_ids.retain(|&symbol_id| symbol_id != VACATED_POSITION);
    }
}
