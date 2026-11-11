//! The parse state: the input as a sequence of placed symbols, every symbol
//! definition ever created, and how often each symbol is placed (its
//! frequency; a symbol with frequency > 0 is a leaf of the Huffman tree).
//!
//! This module is the application stage: it is the only code that changes the
//! state. Everything else reads it.

use crate::fast_hash::FastHashMap;
use crate::huffman_tree::calculate_encoded_symbol_bits;
use crate::predicted_changes::{DeletionPrediction, MergePrediction};
use crate::symbol_definition::SymbolDefinition;

/// Marks a position whose symbol was absorbed into a merge earlier in the
/// current pass; removed by `remove_vacated_positions`.
pub const VACATED_POSITION: u32 = u32::MAX;

pub struct SymbolStream {
    pub symbol_definitions: Vec<SymbolDefinition>,
    pub symbol_frequencies: Vec<u64>,
    pub placed_symbol_ids: Vec<u32>,
    /// Measurement only: parallel to placed_symbol_ids, true where the placed
    /// symbol was produced by a respelling (never read by any decision).
    pub placed_by_respelling: Vec<bool>,
    /// n: the largest window size, and so the longest possible symbol.
    pub maximum_symbol_bit_length: u32,
    pub input_bit_count: u64,
    symbol_id_by_definition: FastHashMap<SymbolDefinition, u32>,
}

/// Number of leaves: symbols with a nonzero frequency.
pub fn count_leaves(symbol_frequencies: &[u64]) -> usize {
    symbol_frequencies.iter().filter(|&&frequency| frequency > 0).count()
}

impl SymbolStream {
    /// Every input bit becomes a placed "0" or "1" symbol.
    pub fn from_input_bytes(input_bytes: &[u8], maximum_symbol_bit_length: u32) -> Self {
        assert!((2..=64).contains(&maximum_symbol_bit_length), "maximum symbol bit length must be in 2..=64");
        let mut placed_symbol_ids = Vec::with_capacity(input_bytes.len() * 8);
        for &byte in input_bytes {
            for bit_position in (0..8).rev() {
                placed_symbol_ids.push(((byte >> bit_position) & 1) as u32);
            }
        }
        let one_count = placed_symbol_ids.iter().filter(|&&symbol_id| symbol_id == 1).count() as u64;
        let zero_count = placed_symbol_ids.len() as u64 - one_count;
        let zero = SymbolDefinition { bits: 0, bit_length: 1 };
        let one = SymbolDefinition { bits: 1, bit_length: 1 };
        let mut symbol_id_by_definition = FastHashMap::default();
        symbol_id_by_definition.insert(zero, 0);
        symbol_id_by_definition.insert(one, 1);
        SymbolStream {
            symbol_definitions: vec![zero, one],
            symbol_frequencies: vec![zero_count, one_count],
            placed_by_respelling: vec![false; placed_symbol_ids.len()],
            placed_symbol_ids,
            maximum_symbol_bit_length,
            input_bit_count: input_bytes.len() as u64 * 8,
            symbol_id_by_definition,
        }
    }

    pub fn leaf_count(&self) -> usize {
        count_leaves(&self.symbol_frequencies)
    }

    /// Encoded symbol bits: exact bits the encoded symbols take up.
    pub fn encoded_symbol_bits(&self) -> u64 {
        calculate_encoded_symbol_bits(&self.symbol_frequencies)
    }

    /// Any symbol ever defined, alive (leaf) or dead (frequency 0).
    pub fn find_symbol_id(&self, definition: SymbolDefinition) -> Option<usize> {
        self.symbol_id_by_definition.get(&definition).map(|&symbol_id| symbol_id as usize)
    }

    pub fn symbol_as_bit_string(&self, symbol_id: usize) -> String {
        self.symbol_definitions[symbol_id].as_bit_string()
    }

    pub fn occupied_position_count(&self) -> usize {
        self.placed_symbol_ids.iter().filter(|&&symbol_id| symbol_id != VACATED_POSITION).count()
    }

    /// Replaces every predicted occurrence with the merged symbol. The first
    /// position of each occurrence holds the merged symbol, the rest are
    /// vacated; all of them are marked consumed for the rest of the pass.
    pub fn apply_merge(&mut self, merge: &MergePrediction, position_consumed_this_pass: &mut [bool]) {
        if merge.merged_symbol_id == self.symbol_definitions.len() {
            self.symbol_definitions.push(merge.merged_definition);
            self.symbol_id_by_definition.insert(merge.merged_definition, merge.merged_symbol_id as u32);
        }
        self.symbol_frequencies = merge.symbol_frequencies_after.clone();
        for &(start_position, end_position) in &merge.merged_occurrence_ranges {
            self.placed_symbol_ids[start_position as usize] = merge.merged_symbol_id as u32;
            self.placed_by_respelling[start_position as usize] = false;
            position_consumed_this_pass[start_position as usize] = true;
            for position in start_position + 1..end_position {
                self.placed_symbol_ids[position as usize] = VACATED_POSITION;
                position_consumed_this_pass[position as usize] = true;
            }
        }
    }

    /// Replaces every placement of each deleted symbol with its respelling,
    /// and drops vacated positions. Positions change, so the pass must end.
    pub fn apply_deletion(&mut self, deletion: &DeletionPrediction) {
        self.symbol_frequencies = deletion.symbol_frequencies_after.clone();
        let mut rebuilt_symbol_ids = Vec::with_capacity(self.placed_symbol_ids.len());
        let mut rebuilt_respelling_marks = Vec::with_capacity(self.placed_symbol_ids.len());
        for (position, &symbol_id) in self.placed_symbol_ids.iter().enumerate() {
            if symbol_id == VACATED_POSITION {
                continue;
            }
            let respelling = deletion
                .respellings
                .iter()
                .find(|respelling| respelling.deleted_symbol_id == symbol_id as usize);
            match respelling {
                Some(respelling) => {
                    rebuilt_symbol_ids.extend_from_slice(&respelling.replacement_symbol_ids);
                    rebuilt_respelling_marks.extend(std::iter::repeat(true).take(respelling.replacement_symbol_ids.len()));
                }
                None => {
                    rebuilt_symbol_ids.push(symbol_id);
                    rebuilt_respelling_marks.push(self.placed_by_respelling[position]);
                }
            }
        }
        self.placed_symbol_ids = rebuilt_symbol_ids;
        self.placed_by_respelling = rebuilt_respelling_marks;
    }

    pub fn remove_vacated_positions(&mut self) {
        let mut kept_count = 0;
        for position in 0..self.placed_symbol_ids.len() {
            if self.placed_symbol_ids[position] != VACATED_POSITION {
                self.placed_symbol_ids[kept_count] = self.placed_symbol_ids[position];
                self.placed_by_respelling[kept_count] = self.placed_by_respelling[position];
                kept_count += 1;
            }
        }
        self.placed_symbol_ids.truncate(kept_count);
        self.placed_by_respelling.truncate(kept_count);
    }
}
